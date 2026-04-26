use std::iter::{repeat_n};
use derive_getters::Getters;
use crate::types::errors::ParseErr;
use crate::types::errors::ParseErr::{InvalidLASTileId, InvalidSubgrid};

const TILE_ID_SEPARATOR: char = '_';
const SUBGRID_ID_RADIX: u32 = 10;

const SUBGRID_TILES_PER_SIDE: u8 = 5;
const SUBGRID_TILE_SIDE_LENGTH_USFT: u16 = 500;
pub const LAS_TILE_SIDE_LENGTH_USFT: u16 = 2500;

const EASTING_BASE_ROLLOVER_POINT: u16 = 1000;
const EASTING_BASE_ROLLOVER_BOUND: u16 = EASTING_BASE_ROLLOVER_POINT / 2;
const PERMITTED_LAS_ID_COMPONENT_MODULI: &[u8] = &[0, 2, 5, 7];

#[derive(Debug)]
// Easting, Northing coordinates (in NYS LI plane) (units of 1000 usft)
pub struct LASTileId(u16, u16);

#[derive(Debug)]
// X, Y (Easting, Northing in units of 500 usft) offset from the SW corner of
// the associated LAS tile
pub struct SubgridId(u8, u8);

#[derive(Debug, Getters)]
pub struct TileId {
    las_tile_id: LASTileId,
    subgrid_id: SubgridId,
}

impl LASTileId {
    pub fn parse(input_str: &str) -> Result<Self, ParseErr> {
        if !input_str.chars().all(|c| c.is_ascii_digit()){
            return Err(InvalidLASTileId);
        }

        if input_str.len() < 1 || input_str.len() > 6 {
            return Err(InvalidLASTileId);
        }

        let northing_start_idx = input_str.len().saturating_sub(3);

        let northing_base: u16 = input_str[northing_start_idx..].parse()?;
        let mut easting_base = match northing_start_idx {
            0 => 1000,
            _ => input_str[..northing_start_idx].parse()?
        };

        if easting_base < EASTING_BASE_ROLLOVER_BOUND {
            easting_base += EASTING_BASE_ROLLOVER_POINT;
        }

        if    !PERMITTED_LAS_ID_COMPONENT_MODULI.contains(&((northing_base % 10) as u8))
            || !PERMITTED_LAS_ID_COMPONENT_MODULI.contains(&((easting_base % 10) as u8)) {
            return Err(InvalidLASTileId);
        }

        Ok(LASTileId(easting_base, northing_base))
    }

    fn easting_id(&self) -> u16 {
        let mut id = self.0;
        if id >= EASTING_BASE_ROLLOVER_POINT {
            id -= EASTING_BASE_ROLLOVER_POINT;
        }
        id
    }

    pub fn id(&self) -> String {
        let easting_id = self.easting_id();
        let northing_id = self.1;
        if easting_id == 0 {
            northing_id.to_string()
        } else {
            format!("{easting_id}{northing_id:03}")
        }
    }

    pub fn ortho_fname(&self) -> String {
        const ZFILL_TO_LENGTH: usize = 6;
        let id_base = self.id().to_string();
        let padding: String = repeat_n('0', ZFILL_TO_LENGTH - id_base.len()).collect();
        format!("{padding}{id_base}.jp2")
    }
}

impl SubgridId {
    pub fn new(x: u8, y: u8) -> Self {
        assert!(x < SUBGRID_TILES_PER_SIDE && y < SUBGRID_TILES_PER_SIDE,
            "SubgridId coordinates must be < {SUBGRID_TILES_PER_SIDE}, got ({x}, {y})");
        SubgridId(x, y)
    }

    pub fn parse(input_str: &str) -> Result<SubgridId, ParseErr> {
        if input_str.len() != 2 {
            return Err(InvalidSubgrid);
        }

        let mut input_str_chars = input_str.chars();
        let (subgrid_x_char, subgrid_y_char) = (
            input_str_chars.next().ok_or(InvalidSubgrid)?,
            input_str_chars.next().ok_or(InvalidSubgrid)?,
        );

        let subgrid_x = subgrid_x_char.to_digit(SUBGRID_ID_RADIX).ok_or(InvalidSubgrid)?;
        let subgrid_y = subgrid_y_char.to_digit(SUBGRID_ID_RADIX).ok_or(InvalidSubgrid)?;

        if subgrid_x >= SUBGRID_TILES_PER_SIDE as u32 || subgrid_y >= SUBGRID_TILES_PER_SIDE as u32 {
            return Err(InvalidSubgrid);
        }

        Ok(SubgridId::new(subgrid_x as u8, subgrid_y as u8))
    }

    // Returns the X, Y, W, H in usft relative to the SW corner of the tile
    pub fn relative_bounds(&self) -> (u16, u16, u16, u16) {
        (
            self.0 as u16 * SUBGRID_TILE_SIDE_LENGTH_USFT,
            self.1 as u16 * SUBGRID_TILE_SIDE_LENGTH_USFT,
            SUBGRID_TILE_SIDE_LENGTH_USFT,
            SUBGRID_TILE_SIDE_LENGTH_USFT
        )
    }
}

impl TileId {
    pub fn parse(input_str: &str) -> Result<TileId, ParseErr> {
        let (las_tile_id_str, subgrid_id_str) = match input_str.find(TILE_ID_SEPARATOR) {
            Some(i) if i > 0 => (&input_str[..i], &input_str[(i + 1)..]),
            _ => return Err(ParseErr::MissingSeparator)
        };

        Ok(
            TileId {
                las_tile_id: LASTileId::parse(las_tile_id_str)?,
                subgrid_id: SubgridId::parse(subgrid_id_str)?
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- LASTileId ---

    #[test]
    fn las_tile_id_valid_typical() {
        let id = LASTileId::parse("500300").unwrap();
        assert_eq!(id.0, 500);
        assert_eq!(id.1, 300);
    }

    #[test]
    fn las_tile_id_valid_permitted_moduli() {
        // moduli 0, 2, 5, 7 are permitted for both components
        // easting 500 (mod 10 = 0), northing 302 (mod 10 = 2)
        let id = LASTileId::parse("500302").unwrap();
        assert_eq!(id.0, 500);
        assert_eq!(id.1, 302);

        // northing ending in 5
        let id = LASTileId::parse("500305").unwrap();
        assert_eq!(id.1, 305);

        // northing ending in 7
        let id = LASTileId::parse("500307").unwrap();
        assert_eq!(id.1, 307);
    }

    #[test]
    fn las_tile_id_valid_easting_rollover() {
        // easting < 500 gets +1000 applied; 200 -> 1200, mod 10 = 0 ✓
        let id = LASTileId::parse("200300").unwrap();
        assert_eq!(id.0, 1200);
        assert_eq!(id.1, 300);
    }

    #[test]
    fn las_tile_id_valid_zero_northing() {
        let id = LASTileId::parse("997000").unwrap();
        assert_eq!(id.0, 997);
        assert_eq!(id.1, 0);
    }
    #[test]
    fn las_tile_id_coordinate_base_rollover() {
        assert_eq!(LASTileId::parse("997125").unwrap().easting_id(), 997);
        assert_eq!(LASTileId::parse("235125").unwrap().easting_id(), 235);
    }

    #[test]
    fn las_tile_id_parse_roundtrip() {
        assert_eq!(LASTileId::parse("997125").unwrap().id(), "997125");
        assert_eq!(LASTileId::parse("235125").unwrap().id(), "235125");
        assert_eq!(LASTileId::parse("125").unwrap().id(), "125");
        assert_eq!(LASTileId::parse("35125").unwrap().id(), "35125");
        assert_eq!(LASTileId::parse("997005").unwrap().id(), "997005");
    }

    #[test]
    fn las_tile_id_ortho_fname() {
        assert_eq!(LASTileId::parse("997125").unwrap().ortho_fname(), "997125.jp2");
        assert_eq!(LASTileId::parse("235125").unwrap().ortho_fname(), "235125.jp2");
        assert_eq!(LASTileId::parse("125").unwrap().ortho_fname(), "000125.jp2");
        assert_eq!(LASTileId::parse("37").unwrap().ortho_fname(), "000037.jp2");
        assert_eq!(LASTileId::parse("35125").unwrap().ortho_fname(), "035125.jp2");
    }

    #[test]
    fn las_tile_id_invalid_non_digit() {
        assert!(matches!(LASTileId::parse("50a300"), Err(InvalidLASTileId)));
    }

    #[test]
    fn las_tile_id_invalid_empty() {
        assert!(matches!(LASTileId::parse(""), Err(InvalidLASTileId)));
    }

    #[test]
    fn las_tile_id_too_big() {
        assert!(matches!(LASTileId::parse("1237337"), Err(InvalidLASTileId)));
    }

    #[test]
    fn las_tile_id_invalid_northing_modulus() {
        // northing 301 mod 10 = 1, not in permitted set
        assert!(matches!(LASTileId::parse("500301"), Err(InvalidLASTileId)));
    }

    #[test]
    fn las_tile_id_invalid_easting_modulus() {
        // easting 501 mod 10 = 1, not in permitted set
        assert!(matches!(LASTileId::parse("501300"), Err(InvalidLASTileId)));
    }

    // --- SubgridId ---

    #[test]
    fn subgrid_id_valid_corners() {
        let id = SubgridId::parse("00").unwrap();
        assert_eq!(id.0, 0);
        assert_eq!(id.1, 0);

        let id = SubgridId::parse("44").unwrap();
        assert_eq!(id.0, 4);
        assert_eq!(id.1, 4);

        let id = SubgridId::parse("04").unwrap();
        assert_eq!(id.0, 0);
        assert_eq!(id.1, 4);
    }

    #[test]
    fn subgrid_id_invalid_out_of_range() {
        assert!(matches!(SubgridId::parse("50"), Err(InvalidSubgrid)));
        assert!(matches!(SubgridId::parse("05"), Err(InvalidSubgrid)));
        assert!(matches!(SubgridId::parse("55"), Err(InvalidSubgrid)));
    }

    #[test]
    fn subgrid_id_invalid_non_digit() {
        assert!(matches!(SubgridId::parse("a0"), Err(InvalidSubgrid)));
        assert!(matches!(SubgridId::parse("0b"), Err(InvalidSubgrid)));
    }

    #[test]
    fn subgrid_id_invalid_wrong_length() {
        assert!(matches!(SubgridId::parse(""), Err(InvalidSubgrid)));
        assert!(matches!(SubgridId::parse("0"), Err(InvalidSubgrid)));
        assert!(matches!(SubgridId::parse("012"), Err(InvalidSubgrid)));
    }

    // --- SubgridId::relative_bounds ---

    #[test]
    fn subgrid_relative_bounds_sw_corner() {
        assert_eq!(SubgridId::new(0, 0).relative_bounds(), (0, 0, 500, 500));
    }

    #[test]
    fn subgrid_relative_bounds_ne_corner() {
        assert_eq!(SubgridId::new(4, 4).relative_bounds(), (2000, 2000, 500, 500));
    }

    #[test]
    fn subgrid_relative_bounds_x_axis() {
        assert_eq!(SubgridId::new(1, 0).relative_bounds(), (500, 0, 500, 500));
        assert_eq!(SubgridId::new(3, 0).relative_bounds(), (1500, 0, 500, 500));
    }

    #[test]
    fn subgrid_relative_bounds_y_axis() {
        assert_eq!(SubgridId::new(0, 1).relative_bounds(), (0, 500, 500, 500));
        assert_eq!(SubgridId::new(0, 3).relative_bounds(), (0, 1500, 500, 500));
    }

    #[test]
    fn subgrid_relative_bounds_middle() {
        assert_eq!(SubgridId::new(2, 3).relative_bounds(), (1000, 1500, 500, 500));
    }

    #[test]
    fn subgrid_relative_bounds_size_is_constant() {
        for x in 0..5u8 {
            for y in 0..5u8 {
                let (_, _, w, h) = SubgridId::new(x, y).relative_bounds();
                assert_eq!((w, h), (500, 500));
            }
        }
    }

    #[test]
    #[should_panic]
    fn subgrid_new_panics_on_invalid_x() {
        SubgridId::new(5, 0);
    }

    #[test]
    #[should_panic]
    fn subgrid_new_panics_on_invalid_y() {
        SubgridId::new(0, 5);
    }

    // --- TileId ---

    #[test]
    fn tile_id_valid() {
        let id = TileId::parse("500300_23").unwrap();
        assert_eq!(id.las_tile_id.0, 500);
        assert_eq!(id.las_tile_id.1, 300);
        assert_eq!(id.subgrid_id.0, 2);
        assert_eq!(id.subgrid_id.1, 3);
    }

    #[test]
    fn tile_id_valid_short_las_id() {
        let id = TileId::parse("235_00").unwrap();
        assert_eq!(id.las_tile_id.0, 1000);
        assert_eq!(id.las_tile_id.1, 235);
        assert_eq!(id.subgrid_id.0, 0);
        assert_eq!(id.subgrid_id.1, 0);
    }

    #[test]
    fn tile_id_missing_separator() {
        assert!(matches!(TileId::parse("500300"), Err(ParseErr::MissingSeparator)));
    }

    #[test]
    fn tile_id_separator_at_start() {
        assert!(matches!(TileId::parse("_500300"), Err(ParseErr::MissingSeparator)));
    }

    #[test]
    fn tile_id_invalid_las_propagates() {
        assert!(matches!(TileId::parse("500301_23"), Err(InvalidLASTileId)));
    }

    #[test]
    fn tile_id_invalid_subgrid_propagates() {
        assert!(matches!(TileId::parse("500300_55"), Err(InvalidSubgrid)));
    }
}