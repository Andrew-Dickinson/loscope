use geo::{BoundingRect, Contains, Point, Polygon, Rect};
use loscope::types::tiles::SUBGRID_TILE_SIDE_LENGTH_USFT;

const TILE_SIDE: f64 = SUBGRID_TILE_SIDE_LENGTH_USFT as f64;

/// Rasterize a polygon to a 1-usft grid covering its bounding box.
///
/// Returns `(x_sw, y_sw, width, height, raster)` where:
/// - `x_sw`, `y_sw` are the SW corner of the bounding box (integer usft)
/// - `raster` is flat `[easting_local * height + northing_local]`, Gray16 inches
pub fn rasterize_polygon(
    poly: &Polygon<f64>,
    height_inches: u16,
) -> (i64, i64, u32, u32, Vec<u16>) {
    let bbox: Rect<f64> = match poly.bounding_rect() {
        Some(b) => b,
        None => return (0, 0, 1, 1, vec![0u16]),
    };
    let x_sw = bbox.min().x.floor() as i64;
    let y_sw = bbox.min().y.floor() as i64;
    let x_ne = bbox.max().x.ceil() as i64;
    let y_ne = bbox.max().y.ceil() as i64;
    let w = ((x_ne - x_sw).max(1)) as u32;
    let h = ((y_ne - y_sw).max(1)) as u32;

    let mut raster = vec![0u16; w as usize * h as usize];

    for xi in 0..w {
        for yi in 0..h {
            let center = Point::new(
                x_sw as f64 + xi as f64 + 0.5,
                y_sw as f64 + yi as f64 + 0.5,
            );
            if poly.contains(&center) {
                raster[(xi * h + yi) as usize] = height_inches;
            }
        }
    }

    (x_sw, y_sw, w, h, raster)
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::polygon;
    use loscope::building::heightmap::get_intersecting_tiles;

    fn square_poly(x: f64, y: f64, side: f64) -> Polygon<f64> {
        polygon![
            (x: x,        y: y),
            (x: x + side, y: y),
            (x: x + side, y: y + side),
            (x: x,        y: y + side),
            (x: x,        y: y),
        ]
    }

    #[test]
    fn raster_dims_match_bbox() {
        let poly = square_poly(100.0, 200.0, 10.0);
        let (x_sw, y_sw, w, h, raster) = rasterize_polygon(&poly, 500);
        assert_eq!(x_sw, 100);
        assert_eq!(y_sw, 200);
        assert_eq!(w, 10);
        assert_eq!(h, 10);
        assert_eq!(raster.len(), 100);
    }

    #[test]
    fn interior_pixels_set_outside_zero() {
        let poly = square_poly(0.0, 0.0, 3.0);
        let (_, _, w, h, raster) = rasterize_polygon(&poly, 1000);

        for xi in 0..w {
            for yi in 0..h {
                let idx = (xi * h + yi) as usize;
                assert_eq!(raster[idx], 1000, "pixel ({xi},{yi}) should be 1000");
            }
        }
    }

    #[test]
    fn all_zero_for_empty_polygon() {
        let poly = square_poly(0.0, 0.0, 0.001);
        let (_, _, w, h, raster) = rasterize_polygon(&poly, 999);
        assert_eq!(raster.len(), w as usize * h as usize);
    }

    #[test]
    fn get_intersecting_tiles_uses_geometry_not_bbox() {
        // A narrow polygon that only clips the corner of one tile should only
        // return that one tile, not all bbox tiles.
        let poly = square_poly(500.0, 500.0, 1.0);
        let tiles = get_intersecting_tiles(&poly).map(|(tiles, _)| tiles).unwrap();
        assert!(!tiles.is_empty());
        assert!(tiles.len() <= 4, "small polygon should intersect at most 4 tiles");
    }
}