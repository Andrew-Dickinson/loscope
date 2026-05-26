use crate::types::tiles::TileId;
use geo::{Contains, Polygon, point};
use ndarray::Array2;

/// Mutates `tile_data` so that every pixel whose NYS coordinate falls
/// inside `footprint` is set to zero. Pixels outside the footprint are unchanged.
///
/// Pixel `[xi, yi]` maps to NYS coordinate
/// `(tile_sw_easting + xi, tile_sw_northing + yi)`.
pub fn zero_footprint_pixels(
    footprint: &Polygon,
    tile_id: TileId,
    tile_data: &mut Array2<u16>,
) {
    let sw = tile_id.get_sw_corner();
    let sw_e = *sw.easting();
    let sw_n = *sw.northing();

    for xi in 0..tile_data.nrows() {
        for yi in 0..tile_data.ncols() {
            if footprint.contains(&point! {
                x: sw_e + xi as f64,
                y: sw_n + yi as f64,
            }) {
                tile_data[[xi, yi]] = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::polygon;

    fn tile() -> TileId {
        // SW corner = (500000, 300000)
        TileId::parse("500300_00").unwrap()
    }

    fn uniform_tile(val: u16, shape: (usize, usize)) -> Array2<u16> {
        Array2::from_elem(shape, val)
    }

    #[test]
    fn pixels_inside_footprint_are_zeroed() {
        // Footprint covers only the 10×10 sub-square starting at (500010, 300010).
        let footprint = polygon![
            (x: 500010.0, y: 300010.0),
            (x: 500020.0, y: 300010.0),
            (x: 500020.0, y: 300020.0),
            (x: 500010.0, y: 300020.0),
            (x: 500010.0, y: 300010.0),
        ];
        let mut data = uniform_tile(500, (50, 50));
        zero_footprint_pixels(&footprint, tile(), &mut data);

        // Pixel at offset (15, 15) maps to NYS (500015, 300015) → inside → 0
        assert_eq!(data[[15, 15]], 0, "inside pixel must be zeroed");
    }

    #[test]
    fn pixels_outside_footprint_are_preserved() {
        let footprint = polygon![
            (x: 500010.0, y: 300010.0),
            (x: 500020.0, y: 300010.0),
            (x: 500020.0, y: 300020.0),
            (x: 500010.0, y: 300020.0),
            (x: 500010.0, y: 300010.0),
        ];
        let mut data = uniform_tile(500, (50, 50));
        zero_footprint_pixels(&footprint, tile(), &mut data);

        // Pixel at offset (0, 0) → NYS (500000, 300000) → outside → unchanged
        assert_eq!(data[[0, 0]], 500, "outside pixel must be unchanged");
        // Pixel at offset (49, 49) → outside → unchanged
        assert_eq!(data[[49, 49]], 500, "outside pixel must be unchanged");
    }

    #[test]
    fn degenerate_footprint_zeroes_nothing() {
        // A zero-area point polygon contains nothing
        let footprint = polygon![
            (x: 500010.0, y: 300010.0),
            (x: 500010.0, y: 300010.0),
        ];
        let mut data = uniform_tile(300, (10, 10));
        zero_footprint_pixels(&footprint, tile(), &mut data);
        assert!(data.iter().all(|&v| v == 300), "degenerate poly must zero nothing");
    }

    #[test]
    fn footprint_covering_full_tile_zeroes_all() {
        // Polygon large enough to contain the entire 500×500 tile
        let footprint = polygon![
            (x: 499999.0, y: 299999.0),
            (x: 500501.0, y: 299999.0),
            (x: 500501.0, y: 300501.0),
            (x: 499999.0, y: 300501.0),
            (x: 499999.0, y: 299999.0),
        ];
        let mut data = uniform_tile(100, (500, 500));
        zero_footprint_pixels(&footprint, tile(), &mut data);
        assert!(data.iter().all(|&v| v == 0), "all pixels must be zeroed");
    }
}
