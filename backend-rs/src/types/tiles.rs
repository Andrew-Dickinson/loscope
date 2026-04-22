use std::fmt::{Display, Formatter};
use rocket::request::FromParam;
use crate::types::errors::ParseErr;
use crate::types::errors::ParseErr::{InvalidLASTileId, InvalidSubgrid};

const TILE_ID_SEPARATOR: char = '_';
const SUBGRID_ID_RADIX: u32 = 10;

const SUBGRID_SIZE: u8 = 5;

const EASTING_BASE_ROLLOVER_POINT: u16 = 1000;
const EASTING_BASE_ROLLOVER_BOUND: u16 = EASTING_BASE_ROLLOVER_POINT / 2;
const PERMITTED_LAS_ID_COMPONENT_MODULI: &[u8] = &[0, 2, 5, 7];

#[derive(Debug)]
pub struct LASTileId(u16, u16);

#[derive(Debug)]
pub struct SubgridId(u8, u8);

#[derive(Debug)]
pub struct TileId {
    las_tile_id: LASTileId,
    subgrid_id: SubgridId,
}

impl LASTileId {
    pub fn parse(input_str: &str) -> Result<Self, ParseErr> {
        if !input_str.chars().all(|c| c.is_ascii_digit()){
            return Err(InvalidLASTileId);
        }

        if input_str.len() < 1 {
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
}

impl SubgridId {
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

        if subgrid_x >= SUBGRID_SIZE as u32 || subgrid_y >= SUBGRID_SIZE as u32 {
            return Err(InvalidSubgrid);
        }

        Ok(SubgridId(subgrid_x as u8, subgrid_y as u8))
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
    fn las_tile_id_invalid_non_digit() {
        assert!(matches!(LASTileId::parse("50a300"), Err(InvalidLASTileId)));
    }

    #[test]
    fn las_tile_id_invalid_empty() {
        assert!(matches!(LASTileId::parse(""), Err(InvalidLASTileId)));
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