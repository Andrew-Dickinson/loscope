use image::{DynamicImage, ImageBuffer, Rgba};
use ndarray::Array2;
use typed_floats::tf64::PositiveFinite;

const TILE_SIDE: usize = 500;
const UPSCALE: usize = 8;
const OUT_SIDE: usize = TILE_SIDE * UPSCALE;

fn interp(v: f64, stops: &[f64], values: &[f64]) -> f64 {
    if v <= stops[0] {
        return values[0];
    }
    let last = stops.len() - 1;
    if v >= stops[last] {
        return values[last];
    }
    for i in 0..last {
        if v <= stops[i + 1] {
            let t = (v - stops[i]) / (stops[i + 1] - stops[i]);
            return values[i] + t * (values[i + 1] - values[i]);
        }
    }
    values[last]
}

/// Converts a 500×500 intersection raster (indexed [easting, northing]) into a
/// 4000×4000 RGBA image using a SunsetDark-inspired colormap, with a 2-pixel
/// black outline around the filled region.
pub fn tile_intersection_to_img(intersection: Array2<Option<&PositiveFinite>>) -> Option<DynamicImage> {
    // SunsetDark-inspired colormap stops
    const STOPS: [f64; 5] = [0.0, 0.4, 0.6, 0.75, 1.0];
    const R: [f64; 5] = [255.0, 255.0, 210.0, 138.0, 214.0];
    const G: [f64; 5] = [215.0, 105.0, 18.0, 0.0, 2.0];
    const B: [f64; 5] = [0.0, 0.0, 28.0, 16.0, 52.0];

    // Build 4000×4000 pixel buffer: upscale 8x, flip north-up.
    // intersection[[x, y]]: x=easting (0..500), y=northing (0 = south).
    // Image row 0 = north (northing=499), col = easting, both repeated 8x.
    if intersection.iter().all(|cell| {
        let Some(val) = cell else { return true };
        **val == 0.0
    }) {
        return None;
    }

    let mut pixels = vec![[0u8; 4]; OUT_SIDE * OUT_SIDE];

    for out_row in 0..OUT_SIDE {
        let y = TILE_SIDE - 1 - out_row / UPSCALE;
        for out_col in 0..OUT_SIDE {
            let x = out_col / UPSCALE;
            if let Some(pf) = intersection[[x, y]] {
                let v = f64::from(*pf);
                if v > 0.0 {
                    pixels[out_row * OUT_SIDE + out_col] = [
                        interp(v, &STOPS, &R) as u8,
                        interp(v, &STOPS, &G) as u8,
                        interp(v, &STOPS, &B) as u8,
                        255,
                    ];
                }
            }
        }
    }

    // 2-pixel black outline: dilate the opaque mask twice (4-connected, wrapping),
    // then paint the border ring (dilated & !filled) black.
    let filled: Vec<bool> = pixels.iter().map(|p| p[3] > 0).collect();
    let mut dilated = filled.clone();

    for _ in 0..2 {
        let prev = dilated.clone();
        for row in 0..OUT_SIDE {
            for col in 0..OUT_SIDE {
                let up   = row.checked_sub(1).map(|r| prev[r * OUT_SIDE + col]).unwrap_or(false);
                let down = if row + 1 < OUT_SIDE { prev[(row + 1) * OUT_SIDE + col] } else { false };
                let left  = col.checked_sub(1).map(|c| prev[row * OUT_SIDE + c]).unwrap_or(false);
                let right = if col + 1 < OUT_SIDE { prev[row * OUT_SIDE + col + 1] } else { false };
                dilated[row * OUT_SIDE + col] =
                    prev[row * OUT_SIDE + col] | up | down | left | right;
            }
        }
    }

    for i in 0..(OUT_SIDE * OUT_SIDE) {
        if dilated[i] && !filled[i] {
            pixels[i] = [0, 0, 0, 255];
        }
    }

    let raw: Vec<u8> = pixels.into_iter().flatten().collect();
    Some(DynamicImage::ImageRgba8(
        ImageBuffer::from_raw(OUT_SIDE as u32, OUT_SIDE as u32, raw)
            .expect("pixel buffer dimensions must match OUT_SIDE²×4"),
    ))
}

#[cfg(test)]
mod tests {
    use ndarray::Array2;
    use typed_floats::tf64::PositiveFinite;
    use super::{tile_intersection_to_img, TILE_SIDE, UPSCALE, OUT_SIDE};

    fn pf(v: f64) -> PositiveFinite { PositiveFinite::new(v).unwrap() }

    fn rgba_at(img: &image::DynamicImage, col: u32, row: u32) -> [u8; 4] {
        use image::GenericImageView;
        let p = img.get_pixel(col, row);
        p.0
    }

    // All-None input → None
    #[test]
    fn all_none_returns_none() {
        let grid: Array2<Option<&PositiveFinite>> = Array2::from_elem((TILE_SIDE, TILE_SIDE), None);
        assert!(tile_intersection_to_img(grid).is_none());
    }

    #[test]
    fn all_zero_returns_none() {
        let zero = Some(PositiveFinite::default());
        let grid: Array2<Option<&PositiveFinite>> = Array2::from_elem((TILE_SIDE, TILE_SIDE), zero.as_ref());
        assert!(tile_intersection_to_img(grid).is_none());
    }

    // A single filled cell at (easting=0, northing=0) (SW corner of the tile) should:
    //   - appear in the bottom-left 8×8 block of the output image (after north-up flip)
    //   - have alpha=255 with the colormap color for v=0 (255, 215, 0)
    //   - be surrounded by a 2-pixel black border
    #[test]
    fn single_cell_color_position_and_border() {
        let v = pf(0.1);
        let mut grid: Array2<Option<&PositiveFinite>> = Array2::from_elem((TILE_SIDE, TILE_SIDE), None);
        grid[[0, 0]] = Some(&v); // easting=0, northing=0 → SW corner

        let img = tile_intersection_to_img(grid).unwrap();

        // SW corner (northing=0) maps to the last 8 rows of the image (north-up flip)
        let filled_row_start = (OUT_SIDE - UPSCALE) as u32;
        let filled_col_start = 0u32;

        // Filled block has the expected color for v=0.1:
        for row in filled_row_start..filled_row_start + UPSCALE as u32 {
            for col in filled_col_start..filled_col_start + UPSCALE as u32 {
                let px = rgba_at(&img, col, row);
                assert_eq!(px, [255, 187, 0, 255], "filled pixel ({col},{row}) wrong color");
            }
        }

        // The pixel immediately above the filled block should be black (border)
        let border_row = filled_row_start - 1;
        let border_col = filled_col_start + UPSCALE as u32 / 2;
        let border_px = rgba_at(&img, border_col, border_row);
        assert_eq!(border_px, [0, 0, 0, 255], "border pixel should be black");

        // A pixel 3 rows above the filled block should be transparent (beyond border)
        let far_row = filled_row_start - 3;
        let far_px = rgba_at(&img, border_col, far_row);
        assert_eq!(far_px[3], 0, "pixel beyond border should be transparent");
    }

    // Zero-value cells must render as transparent, not as the v=0 colormap color.
    #[test]
    fn zero_value_pixels_are_transparent() {
        let zero = pf(0.0);
        let nonzero = pf(0.5);
        let mut grid: Array2<Option<&PositiveFinite>> = Array2::from_elem((TILE_SIDE, TILE_SIDE), None);
        grid[[1, 1]] = Some(&zero);     // easting=1,  northing=1 → should be transparent
        grid[[20, 1]] = Some(&nonzero); // easting=20, northing=1 → should be opaque (far away)

        let img = tile_intersection_to_img(grid).unwrap();

        // northing=1 → image row (TILE_SIDE-1-1)*UPSCALE = 498*8 = 3984
        let row = ((TILE_SIDE - 2) * UPSCALE) as u32;

        // zero cell: easting=1 → cols 8..16, well clear of the nonzero cell's border
        for col in 8u32..16 {
            assert_eq!(rgba_at(&img, col, row)[3], 0,
                "zero-value pixel at col {col} should be transparent");
        }

        // non-zero cell: easting=20 → cols 160..168
        for col in 160u32..168 {
            assert_eq!(rgba_at(&img, col, row)[3], 255,
                "non-zero pixel at col {col} should be opaque");
        }
    }

    // A filled cell at the north edge of the tile (northing=499 → image row 0) must not
    // produce spurious black border pixels at the opposite (south) edge of the image.
    #[test]
    fn border_dilation_does_not_wrap_across_image_edges() {
        let v = pf(0.5);
        let mut grid: Array2<Option<&PositiveFinite>> = Array2::from_elem((TILE_SIDE, TILE_SIDE), None);
        grid[[0, TILE_SIDE - 1]] = Some(&v); // easting=0, northing=499 → image row 0, col 0

        let img = tile_intersection_to_img(grid).unwrap();

        // The filled 8×8 block sits at the top-left of the image (rows 0..8, cols 0..8).
        // With wrapping dilation the border would spill to the bottom edge — verify it doesn't.
        for col in 0..OUT_SIDE as u32 {
            let px = rgba_at(&img, col, OUT_SIDE as u32 - 1);
            assert_eq!(px[3], 0, "south edge col {col} should be transparent, not a wrapped border");
        }
        for row in 0..OUT_SIDE as u32 {
            let px = rgba_at(&img, OUT_SIDE as u32 - 1, row);
            assert_eq!(px[3], 0, "east edge row {row} should be transparent, not a wrapped border");
        }
    }
}
