use image::{DynamicImage, ImageBuffer, Rgba};
use ndarray::Array2;
use typed_floats::tf64::PositiveFinite;

const TILE_SIDE: usize = 500;
const UPSCALE: usize = 8;
const OUT_SIDE: usize = TILE_SIDE * UPSCALE;
const BORDER: usize = 2;

fn colormap(v: f64) -> [u8; 4] {
    const STOPS: [f64; 5] = [0.0, 0.4, 0.6, 0.75, 1.0];
    const R: [f64; 5] = [255.0, 255.0, 210.0, 138.0, 214.0];
    const G: [f64; 5] = [215.0, 105.0, 18.0,   0.0,   2.0];
    const B: [f64; 5] = [  0.0,   0.0, 28.0,  16.0,  52.0];
    const LAST: usize = STOPS.len() - 1;
    if v <= STOPS[0] {
        return [R[0] as u8, G[0] as u8, B[0] as u8, 255];
    }
    if v >= STOPS[LAST] {
        return [R[LAST] as u8, G[LAST] as u8, B[LAST] as u8, 255];
    }
    for i in 0..LAST {
        if v <= STOPS[i + 1] {
            let t = (v - STOPS[i]) / (STOPS[i + 1] - STOPS[i]);
            return [
                (R[i] + t * (R[i + 1] - R[i])) as u8,
                (G[i] + t * (G[i + 1] - G[i])) as u8,
                (B[i] + t * (B[i + 1] - B[i])) as u8,
                255,
            ];
        }
    }
    [R[LAST] as u8, G[LAST] as u8, B[LAST] as u8, 255]
}

#[inline(always)]
fn write_px(raw: &mut [u8], idx: usize, color: [u8; 4]) {
    let off = idx * 4;
    raw[off]     = color[0];
    raw[off + 1] = color[1];
    raw[off + 2] = color[2];
    raw[off + 3] = color[3];
}

/// Converts a 500×500 intersection raster (indexed [easting, northing]) into a
/// 4000×4000 RGBA image using a SunsetDark-inspired colormap, with a 2-pixel
/// black outline around the filled region.
pub fn tile_intersection_to_img(intersection: Array2<Option<&PositiveFinite>>) -> Option<DynamicImage> {
    // Build 4000×4000 pixel buffer: upscale 8x, flip north-up.
    // intersection[[x, y]]: x=easting (0..500), y=northing (0 = south).
    // Image row 0 = north (northing=499), col = easting, both repeated 8x.
    if intersection.iter().all(|cell| {
        let Some(val) = cell else { return true };
        **val == 0.0
    }) {
        return None;
    }

    let mut raw = vec![0u8; OUT_SIDE * OUT_SIDE * 4];
    let mut source_filled = vec![false; TILE_SIDE * TILE_SIDE];

    // Iterate source cells once, writing each color to its 8×8 output block.
    // This computes colormap values 64× less often than the original output-pixel loop.
    for sy in 0..TILE_SIDE {
        let out_row_base = (TILE_SIDE - 1 - sy) * UPSCALE;
        for sx in 0..TILE_SIDE {
            let Some(pf) = intersection[[sx, sy]] else { continue };
            let v = f64::from(*pf);
            if v == 0.0 { continue; }

            source_filled[sy * TILE_SIDE + sx] = true;

            let color = colormap(v);
            let out_col_base = sx * UPSCALE;
            for dr in 0..UPSCALE {
                let row_off = (out_row_base + dr) * OUT_SIDE;
                for dc in 0..UPSCALE {
                    write_px(&mut raw, row_off + out_col_base + dc, color);
                }
            }
        }
    }

    // 2-pixel black border via source-space dilation.
    //
    // A non-filled source cell's output block needs border pixels wherever it is
    // within output-pixel Manhattan distance BORDER of a filled block. Since
    // UPSCALE=8 >> BORDER=2, all reachable pixels fall within one source cell:
    //   - axis-aligned neighbor filled → last/first BORDER cols/rows of our block
    //   - diagonal neighbor filled → exactly the single corner pixel (distance 1+1=2)
    let sf = |sx: isize, sy: isize| -> bool {
        if sx < 0 || sy < 0 || sx >= TILE_SIDE as isize || sy >= TILE_SIDE as isize {
            return false;
        }
        source_filled[sy as usize * TILE_SIDE + sx as usize]
    };

    for sy in 0..TILE_SIDE {
        let out_row_base = (TILE_SIDE - 1 - sy) * UPSCALE;
        for sx in 0..TILE_SIDE {
            if source_filled[sy * TILE_SIDE + sx] { continue; }
            let (isx, isy) = (sx as isize, sy as isize);
            let out_col_base = sx * UPSCALE;

            // right neighbor → last BORDER cols
            if sf(isx + 1, isy) {
                for dr in 0..UPSCALE {
                    let row_off = (out_row_base + dr) * OUT_SIDE;
                    for dc in (UPSCALE - BORDER)..UPSCALE {
                        write_px(&mut raw, row_off + out_col_base + dc, [0, 0, 0, 255]);
                    }
                }
            }
            // left neighbor → first BORDER cols
            if sf(isx - 1, isy) {
                for dr in 0..UPSCALE {
                    let row_off = (out_row_base + dr) * OUT_SIDE;
                    for dc in 0..BORDER {
                        write_px(&mut raw, row_off + out_col_base + dc, [0, 0, 0, 255]);
                    }
                }
            }
            // image-up neighbor (northing+1) → first BORDER rows
            if sf(isx, isy + 1) {
                for dr in 0..BORDER {
                    let row_off = (out_row_base + dr) * OUT_SIDE;
                    for dc in 0..UPSCALE {
                        write_px(&mut raw, row_off + out_col_base + dc, [0, 0, 0, 255]);
                    }
                }
            }
            // image-down neighbor (northing-1) → last BORDER rows
            if sf(isx, isy - 1) {
                for dr in (UPSCALE - BORDER)..UPSCALE {
                    let row_off = (out_row_base + dr) * OUT_SIDE;
                    for dc in 0..UPSCALE {
                        write_px(&mut raw, row_off + out_col_base + dc, [0, 0, 0, 255]);
                    }
                }
            }
            // diagonal neighbors → single corner pixel each (Manhattan distance 1+1=2)
            if sf(isx + 1, isy + 1) { // upper-right: corner (UPSCALE-1, 0)
                write_px(&mut raw, out_row_base * OUT_SIDE + out_col_base + UPSCALE - 1, [0, 0, 0, 255]);
            }
            if sf(isx - 1, isy + 1) { // upper-left: corner (0, 0)
                write_px(&mut raw, out_row_base * OUT_SIDE + out_col_base, [0, 0, 0, 255]);
            }
            if sf(isx + 1, isy - 1) { // lower-right: corner (UPSCALE-1, UPSCALE-1)
                write_px(&mut raw, (out_row_base + UPSCALE - 1) * OUT_SIDE + out_col_base + UPSCALE - 1, [0, 0, 0, 255]);
            }
            if sf(isx - 1, isy - 1) { // lower-left: corner (0, UPSCALE-1)
                write_px(&mut raw, (out_row_base + UPSCALE - 1) * OUT_SIDE + out_col_base, [0, 0, 0, 255]);
            }
        }
    }

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
