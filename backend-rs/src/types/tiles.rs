include!(concat!(env!("OUT_DIR"), "/nyc_tile_set.rs"));

use crate::types::coords::{MAX_NYS_COORD_VALUE, NYSCoords2, valid_nys_coordinate};
use crate::types::errors::TileParseErr;
use crate::types::errors::TileParseErr::{InvalidLASTileId, InvalidSubgrid};
use derive_getters::Getters;
use geo::{Coord, Rect, coord};
use rocket::serde::de::{Error, Visitor};
use rocket::serde::{Deserializer, Serializer};
use serde::de::Unexpected;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fmt::{Display, Formatter};
use std::iter::repeat_n;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use wincode::{SchemaRead, SchemaWrite};

const TILE_ID_SEPARATOR: char = '_';
const SUBGRID_ID_RADIX: u32 = 10;

const SUBGRID_TILES_PER_SIDE: u8 = 5;
pub const SUBGRID_TILE_SIDE_LENGTH_USFT: u16 = 500;
pub const LAS_TILE_SIDE_LENGTH_USFT: u16 = 2500;

const EASTING_BASE_ROLLOVER_POINT: u16 = 1000;
const EASTING_BASE_ROLLOVER_BOUND: u16 = EASTING_BASE_ROLLOVER_POINT / 2;
const PERMITTED_LAS_ID_COMPONENT_MODULI: &[u8] = &[0, 2, 5, 7];

const LAS_ID_UNIT_MUTIPLIER_TO_COORD: u16 = 1000;

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq, SchemaWrite, SchemaRead)]
#[repr(C)]
// Easting, Northing coordinates (in NYS LI plane) (units of 1000 usft)
pub struct LASTileId(u16, u16);

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq, SchemaWrite, SchemaRead)]
#[repr(C)]
// X, Y (Easting, Northing in units of 500 usft) offset from the SW corner of
// the associated LAS tile
pub struct SubgridId(u8, u8);

#[derive(Debug, Getters, Clone, Copy, Eq, Hash, PartialEq, SchemaWrite, SchemaRead)]
pub struct TileId {
    las_tile_id: LASTileId,
    subgrid_id: SubgridId,
}

impl LASTileId {
    pub fn new(easting_base: u16, northing_base: u16) -> Self {
        // Safety: the below unwraps() will never panic, because the outcome of % 10 will always
        // fit into a u8
        assert!(
            PERMITTED_LAS_ID_COMPONENT_MODULI.contains(&((northing_base % 10).try_into().unwrap())),
            "Northing base % 10 must be one of PERMITTED_LAS_ID_COMPONENT_MODULI"
        );
        assert!(
            PERMITTED_LAS_ID_COMPONENT_MODULI.contains(&((easting_base % 10).try_into().unwrap())),
            "Easting base % 10 must be one of PERMITTED_LAS_ID_COMPONENT_MODULI"
        );

        let res = LASTileId(easting_base, northing_base);
        let corner_coords = res.get_sw_corner();

        assert!(
            valid_nys_coordinate(*corner_coords.northing()),
            "Northing must have a value which places the tile between \
                MIN_NYS_COORD_VALUE and MAX_NYS_COORD_VALUE"
        );
        assert!(
            valid_nys_coordinate(*corner_coords.easting()),
            "Easting must have a value which places the tile between \
                MIN_NYS_COORD_VALUE and MAX_NYS_COORD_VALUE"
        );

        res
    }

    pub fn parse(input_str: &str) -> Result<Self, TileParseErr> {
        if !input_str.chars().all(|c| c.is_ascii_digit()) {
            return Err(InvalidLASTileId);
        }

        if input_str.is_empty() || input_str.len() > 6 {
            return Err(InvalidLASTileId);
        }

        let northing_start_idx = input_str.len().saturating_sub(3);

        let northing_base: u16 = input_str[northing_start_idx..].parse()?;
        let mut easting_base = match northing_start_idx {
            0 => 1000,
            _ => input_str[..northing_start_idx].parse()?,
        };

        if easting_base < EASTING_BASE_ROLLOVER_BOUND {
            easting_base += EASTING_BASE_ROLLOVER_POINT;
        }

        // Safety: the below unwraps() will never panic, because the outcome of % 10 will always
        // fit into a u8
        if !PERMITTED_LAS_ID_COMPONENT_MODULI.contains(&((northing_base % 10).try_into().unwrap()))
            || !PERMITTED_LAS_ID_COMPONENT_MODULI
                .contains(&((easting_base % 10).try_into().unwrap()))
        {
            return Err(InvalidLASTileId);
        }

        Ok(LASTileId::new(easting_base, northing_base))
    }

    fn easting_id(&self) -> u16 {
        let mut id = self.0;
        if id >= EASTING_BASE_ROLLOVER_POINT {
            id -= EASTING_BASE_ROLLOVER_POINT;
        }
        id
    }

    pub fn ortho_fname(&self) -> String {
        const ZFILL_TO_LENGTH: usize = 6;
        let id_base = self.to_string();
        let padding: String = repeat_n('0', ZFILL_TO_LENGTH - id_base.len()).collect();
        format!("{padding}{id_base}.jp2")
    }

    pub fn get_sw_corner(&self) -> NYSCoords2 {
        NYSCoords2::new(
            Self::component_to_usft(self.0).into(),
            Self::component_to_usft(self.1).into(),
        )
    }
    pub fn get_sw_corner_u32(&self) -> Coord<u32> {
        coord! {
            x: Self::component_to_usft(self.0),
            y: Self::component_to_usft(self.1)
        }
    }

    // IDs ending in 2 or 7 are truncated and sit 500 usft past a round 1000-usft boundary to land
    // on the 2500-usft tile grid: 0→0, 2→2500, 5→5000, 7→7500, 10→10000, …
    fn component_to_usft(component: u16) -> u32 {
        let base = u32::from(component) * u32::from(LAS_ID_UNIT_MUTIPLIER_TO_COORD);
        if component % 10 == 2 || component % 10 == 7 {
            base + 500
        } else {
            base
        }
    }

    // Inverse of component_to_usft: returns the component whose tile SW corner is at or
    // below `usft`. Tiles repeat every 2500 usft in a 4-step cycle (→ mod 0, 2, 5, 7).
    fn usft_to_component(usft: f64) -> u16 {
        assert!(
            (0.0..=MAX_NYS_COORD_VALUE).contains(&usft),
            "Invalid value for coord conversion {}",
            usft
        );
        /// Safety: tile_block will not overflow, since due to the above
        /// assertion, max(tile_block) is 800, and:
        const _: () = assert!(
            MAX_NYS_COORD_VALUE as u64 / LAS_TILE_SIDE_LENGTH_USFT as u64 <= 800,
            "tile_block would overflow u16 below"
        );
        let tile_block = (usft / f64::from(LAS_TILE_SIDE_LENGTH_USFT)).floor() as u16;
        let major = tile_block / 4;
        let remainder: u16 = match tile_block % 4 {
            0 => 0,
            1 => 2,
            2 => 5,
            _ => 7,
        };
        major * 10 + remainder
    }
}

impl Display for LASTileId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let easting_id = self.easting_id();
        let northing_id = self.1;
        if easting_id == 0 {
            write!(f, "{}", northing_id)
        } else {
            write!(f, "{easting_id}{northing_id:03}")
        }
    }
}

impl SubgridId {
    pub fn new(x: u8, y: u8) -> Self {
        assert!(
            x < SUBGRID_TILES_PER_SIDE && y < SUBGRID_TILES_PER_SIDE,
            "SubgridId coordinates must be < {SUBGRID_TILES_PER_SIDE}, got ({x}, {y})"
        );
        SubgridId(x, y)
    }

    pub fn parse(input_str: &str) -> Result<SubgridId, TileParseErr> {
        if input_str.len() != 2 {
            return Err(InvalidSubgrid);
        }

        let mut input_str_chars = input_str.chars();
        let (subgrid_x_char, subgrid_y_char) = (
            input_str_chars.next().ok_or(InvalidSubgrid)?,
            input_str_chars.next().ok_or(InvalidSubgrid)?,
        );

        let subgrid_x = subgrid_x_char
            .to_digit(SUBGRID_ID_RADIX)
            .ok_or(InvalidSubgrid)?;
        let subgrid_y = subgrid_y_char
            .to_digit(SUBGRID_ID_RADIX)
            .ok_or(InvalidSubgrid)?;

        let side_len_u32: u32 = SUBGRID_TILES_PER_SIDE.into();
        if subgrid_x >= side_len_u32 || subgrid_y >= side_len_u32 {
            return Err(InvalidSubgrid);
        }

        // Safety: the unwrap() calls below are safe because SUBGRID_TILES_PER_SIDE << max(u8)
        // and we just validated these are both < SUBGRID_TILES_PER_SIDE
        Ok(SubgridId::new(
            subgrid_x.try_into().unwrap(),
            subgrid_y.try_into().unwrap(),
        ))
    }

    // Returns the X, Y, W, H in usft relative to the SW corner of the tile
    pub fn relative_bounds(&self) -> (u16, u16, u16, u16) {
        let e_u16: u16 = self.0.into();
        let n_u16: u16 = self.1.into();
        (
            e_u16 * SUBGRID_TILE_SIDE_LENGTH_USFT,
            n_u16 * SUBGRID_TILE_SIDE_LENGTH_USFT,
            SUBGRID_TILE_SIDE_LENGTH_USFT,
            SUBGRID_TILE_SIDE_LENGTH_USFT,
        )
    }
}

impl Display for SubgridId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.0, self.1)
    }
}

impl TileId {
    pub fn new(las_tile_id: LASTileId, subgrid_id: SubgridId) -> Self {
        TileId { las_tile_id, subgrid_id }
    }

    pub fn parse(input_str: &str) -> Result<TileId, TileParseErr> {
        let (las_tile_id_str, subgrid_id_str) = match input_str.find(TILE_ID_SEPARATOR) {
            Some(i) if i > 0 => (&input_str[..i], &input_str[(i + 1)..]),
            _ => return Err(TileParseErr::MissingSeparator),
        };

        Ok(TileId {
            las_tile_id: LASTileId::parse(las_tile_id_str)?,
            subgrid_id: SubgridId::parse(subgrid_id_str)?,
        })
    }

    pub fn tiff_fname(&self) -> String {
        self.tiff_fname_with_suffix("")
    }

    pub fn tiff_fname_with_suffix(&self, suffix: &str) -> String {
        format!("{self}{suffix}.tif")
    }

    pub fn get_sw_corner(&self) -> NYSCoords2 {
        let las_corner = self.las_tile_id.get_sw_corner();
        let (offset_e, offset_n, _, _) = self.subgrid_id.relative_bounds();
        let offset_e: f64 = offset_e.into();
        let offset_n: f64 = offset_n.into();

        NYSCoords2::new(
            *las_corner.easting() + offset_e,
            *las_corner.northing() + offset_n,
        )
    }

    pub fn get_bounds(&self) -> Rect<u32> {
        let las_corner = self.las_tile_id.get_sw_corner_u32();
        let (offset_e, offset_n, height, width) = self.subgrid_id.relative_bounds();

        let w = las_corner.x + u32::from(offset_e);
        let s = las_corner.y + u32::from(offset_n);
        let n = las_corner.y + u32::from(offset_n) + u32::from(height);
        let e = las_corner.x + u32::from(offset_e) + u32::from(width);

        Rect::new(coord! {x: w, y: s}, coord! {x: e, y: n})
    }

    pub fn from_contained_point(coords: &NYSCoords2) -> Self {
        let easting = *coords.easting();
        let northing = *coords.northing();

        let las_tile_id = LASTileId::new(
            LASTileId::usft_to_component(easting),
            LASTileId::usft_to_component(northing),
        );

        let las_sw = las_tile_id.get_sw_corner();
        let offset_e = easting - *las_sw.easting();
        let offset_n = northing - *las_sw.northing();

        let subgrid_x = (offset_e / f64::from(SUBGRID_TILE_SIDE_LENGTH_USFT)).floor();
        let subgrid_y = (offset_n / f64::from(SUBGRID_TILE_SIDE_LENGTH_USFT)).floor();

        // Safety: subgrid_x and subgrid_y are bounded to 2500.0/500.0 = 5, making the as
        // coercions below safe
        assert!(subgrid_y < 5.0);
        assert!(subgrid_x < 5.0);
        TileId {
            las_tile_id,
            subgrid_id: SubgridId::new(subgrid_x as u8, subgrid_y as u8),
        }
    }

    pub fn from_adjacent_tile(starting: &TileId, offset: (isize, isize)) -> Self {
        let starting_sw = starting.las_tile_id().get_sw_corner();
        let starting_s = starting_sw.northing().floor() as isize;
        let starting_w = starting_sw.easting().floor() as isize;

        let total_x_tiles_from_las_corner = isize::from(starting.subgrid_id.0) + offset.0;
        let total_y_tiles_from_las_corner = isize::from(starting.subgrid_id.1) + offset.1;

        TileId {
            las_tile_id: LASTileId::new(
                // Safety: the following unwraps are safe because
                // SUBGRID_TILE_SIDE_LENGTH_USFT = 500 fits into an isize (2**32) no problem, on
                // any 32+ bit system
                LASTileId::usft_to_component(
                    (starting_w
                        + isize::try_from(SUBGRID_TILE_SIDE_LENGTH_USFT).unwrap()
                            * total_x_tiles_from_las_corner) as f64,
                ),
                LASTileId::usft_to_component(
                    (starting_s
                        + isize::try_from(SUBGRID_TILE_SIDE_LENGTH_USFT).unwrap()
                            * total_y_tiles_from_las_corner) as f64,
                ),
            ),
            subgrid_id: SubgridId::new(
                // Safety: the following unwraps are safe because SUBGRID_TILES_PER_SIDE is a u8,
                // therefore x % SUBGRID_TILES_PER_SIDE fits in a u8 for all x ∈ ℤ
                u8::try_from(
                    total_x_tiles_from_las_corner.rem_euclid(isize::from(SUBGRID_TILES_PER_SIDE)),
                )
                .unwrap(),
                u8::try_from(
                    total_y_tiles_from_las_corner.rem_euclid(isize::from(SUBGRID_TILES_PER_SIDE)),
                )
                .unwrap(),
            ),
        }
    }

    pub fn as_packed_u64(&self) -> u64 {
        (self.las_tile_id.0 as u64)
            | ((self.las_tile_id.1 as u64) << 16)
            | ((self.subgrid_id.0 as u64) << 32)
            | ((self.subgrid_id.1 as u64) << 40)
    }

    pub fn is_in_nyc(&self) -> bool {
        NYC_TILE_SET.binary_search(&self.as_packed_u64()).is_ok()
    }
}

impl Display for TileId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}_{}", self.las_tile_id, self.subgrid_id)
    }
}

impl Serialize for TileId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TileId {
    fn deserialize<D: Deserializer<'de>>(des: D) -> Result<Self, D::Error> {
        pub struct TileIdVisitor;
        impl<'de> Visitor<'de> for TileIdVisitor {
            type Value = TileId;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a tile id in the form <las_tile_id>_<subgrid_id>")
            }
            fn visit_str<E>(self, input_str: &str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                TileId::parse(input_str)
                    .map_err(|_| Error::invalid_type(Unexpected::Str(input_str), &self))
            }
        }

        des.deserialize_str(TileIdVisitor)
    }
}


#[derive(Clone, Copy, PartialEq, Eq, Debug, TryFromPrimitive, IntoPrimitive)]
#[repr(u8)]
pub enum TerrainClass {
    None = 0,
    Vegetation = 1,
    Building = 2,
    Water = 3,
}


impl Default for TerrainClass {
    fn default() -> Self {
        TerrainClass::Water
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
        assert_eq!(LASTileId::parse("997125").unwrap().to_string(), "997125");
        assert_eq!(LASTileId::parse("235125").unwrap().to_string(), "235125");
        assert_eq!(LASTileId::parse("125").unwrap().to_string(), "125");
        assert_eq!(LASTileId::parse("35125").unwrap().to_string(), "35125");
        assert_eq!(LASTileId::parse("997005").unwrap().to_string(), "997005");
    }

    #[test]
    fn las_tile_id_ortho_fname() {
        assert_eq!(
            LASTileId::parse("997125").unwrap().ortho_fname(),
            "997125.jp2"
        );
        assert_eq!(
            LASTileId::parse("235125").unwrap().ortho_fname(),
            "235125.jp2"
        );
        assert_eq!(LASTileId::parse("125").unwrap().ortho_fname(), "000125.jp2");
        assert_eq!(LASTileId::parse("37").unwrap().ortho_fname(), "000037.jp2");
        assert_eq!(
            LASTileId::parse("35125").unwrap().ortho_fname(),
            "035125.jp2"
        );
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
        assert_eq!(
            SubgridId::new(4, 4).relative_bounds(),
            (2000, 2000, 500, 500)
        );
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
        assert_eq!(
            SubgridId::new(2, 3).relative_bounds(),
            (1000, 1500, 500, 500)
        );
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
        assert!(matches!(
            TileId::parse("500300"),
            Err(TileParseErr::MissingSeparator)
        ));
    }

    #[test]
    fn tile_id_separator_at_start() {
        assert!(matches!(
            TileId::parse("_500300"),
            Err(TileParseErr::MissingSeparator)
        ));
    }

    #[test]
    fn tile_id_invalid_las_propagates() {
        assert!(matches!(TileId::parse("500301_23"), Err(InvalidLASTileId)));
    }

    #[test]
    fn tile_id_invalid_subgrid_propagates() {
        assert!(matches!(TileId::parse("500300_55"), Err(InvalidSubgrid)));
    }

    // --- LASTileId::get_sw_corner ---

    #[test]
    fn las_tile_id_sw_corner_typical() {
        let corner = LASTileId::parse("980170").unwrap().get_sw_corner();
        assert_eq!(*corner.easting(), 980_000.0);
        assert_eq!(*corner.northing(), 170_000.0);
    }

    #[test]
    fn las_tile_id_sw_corner_short_id() {
        let corner = LASTileId::parse("150").unwrap().get_sw_corner();
        assert_eq!(*corner.easting(), 1_000_000.0);
        assert_eq!(*corner.northing(), 150_000.0);
    }

    #[test]
    fn las_tile_id_sw_corner_mid_offset_2() {
        let corner = LASTileId::parse("2152").unwrap().get_sw_corner();
        assert_eq!(*corner.easting(), 1_002_500.0);
        assert_eq!(*corner.northing(), 152_500.0);
    }

    #[test]
    fn las_tile_id_sw_corner_mid_offset_7() {
        let corner = LASTileId::parse("987177").unwrap().get_sw_corner();
        assert_eq!(*corner.easting(), 987_500.0);
        assert_eq!(*corner.northing(), 177_500.0);
    }

    // --- Display for SubgridId ---

    #[test]
    fn subgrid_id_display() {
        assert_eq!(SubgridId::new(0, 0).to_string(), "00");
        assert_eq!(SubgridId::new(2, 3).to_string(), "23");
        assert_eq!(SubgridId::new(4, 4).to_string(), "44");
        assert_eq!(SubgridId::new(0, 4).to_string(), "04");
    }

    // --- Display for TileId ---

    #[test]
    fn tile_id_display_roundtrip() {
        assert_eq!(TileId::parse("500300_23").unwrap().to_string(), "500300_23");
        assert_eq!(TileId::parse("235_00").unwrap().to_string(), "235_00");
        assert_eq!(TileId::parse("997005_44").unwrap().to_string(), "997005_44");
        assert_eq!(TileId::parse("987177_42").unwrap().to_string(), "987177_42");
    }

    // --- TileId::tiff_fname ---

    #[test]
    fn tile_id_tiff_fname() {
        assert_eq!(
            TileId::parse("500300_23").unwrap().tiff_fname(),
            "500300_23.tif"
        );
        assert_eq!(TileId::parse("125_00").unwrap().tiff_fname(), "125_00.tif");
    }

    // --- TileId::get_sw_corner ---

    #[test]
    fn tile_id_sw_corner_no_offset() {
        // subgrid 00 → no offset, result equals LAS corner
        let corner = TileId::parse("500300_00").unwrap().get_sw_corner();
        assert_eq!(*corner.easting(), 500_000.0);
        assert_eq!(*corner.northing(), 300_000.0);
    }

    #[test]
    fn tile_id_sw_corner_with_offset() {
        // LAS 500300 → base (500000, 300000); subgrid 23 → x=2,y=3 → offset (1000, 1500)
        let corner = TileId::parse("500300_23").unwrap().get_sw_corner();
        assert_eq!(*corner.easting(), 501_000.0);
        assert_eq!(*corner.northing(), 301_500.0);
    }

    #[test]
    fn las_tile_id_sw_corner_mid_offset_7_23() {
        let corner = TileId::parse("987177_23").unwrap().get_sw_corner();
        assert_eq!(*corner.easting(), 988_500.0);
        assert_eq!(*corner.northing(), 179_000.0);
    }

    #[test]
    fn tile_id_sw_corner_max_subgrid() {
        // subgrid 44 → offset (2000, 2000)
        let corner = TileId::parse("500300_44").unwrap().get_sw_corner();
        assert_eq!(*corner.easting(), 502_000.0);
        assert_eq!(*corner.northing(), 302_000.0);
    }

    // --- TileId::get_bounds ---
    #[test]
    fn tile_id_get_bounds_sw_subgrid() {
        // LAS 500300, subgrid 00: tile from (500000, 300000) to (500500, 300500)
        let bounds = &TileId::parse("500300_00").unwrap().get_bounds();
        assert_eq!(bounds.min().x_y(), (500_000, 300_000)); // SW
        assert_eq!(bounds.max().x_y(), (500_500, 300_500)); // NE
    }

    #[test]
    fn tile_id_get_bounds_sw_subgrid_mid_offset_7_23() {
        let bounds = &TileId::parse("987177_23").unwrap().get_bounds();
        assert_eq!(bounds.min().x_y(), (988_500, 179_000)); // SW
        assert_eq!(bounds.max().x_y(), (989_000, 179_500)); // NE
    }

    #[test]
    fn tile_id_get_bounds_inner_subgrid() {
        // LAS 500300, subgrid 23: SW=(501000, 301500), NE=(501500, 302000)
        let bounds = &TileId::parse("500300_23").unwrap().get_bounds();
        assert_eq!(bounds.min().x_y(), (501_000, 301_500)); // SW
        assert_eq!(bounds.max().x_y(), (501_500, 302_000)); // NE
    }

    #[test]
    fn tile_id_get_bounds_is_500_usft_square() {
        // All subgrid tiles should be exactly 500×500 usft
        for x in 0..5u8 {
            for y in 0..5u8 {
                let id = format!("500300_{x}{y}");
                let bounds = TileId::parse(&id).unwrap().get_bounds();
                assert_eq!(
                    (bounds.width(), bounds.height()),
                    (500, 500),
                    "failed for subgrid {x}{y}"
                );
            }
        }
    }

    // --- TileId::from_contained_point ---

    #[test]
    fn from_contained_point_sw_corner() {
        // SW corner of a tile belongs to that tile
        let tile = TileId::parse("990200_23").unwrap();
        let result = TileId::from_contained_point(&tile.get_sw_corner());
        assert_eq!(result.to_string(), "990200_23");
    }

    #[test]
    fn from_contained_point_example() {
        // SW corner of a tile belongs to that tile
        let _tile = TileId::parse("982182_00").unwrap();
        let result = TileId::from_contained_point(&NYSCoords2::new(982634.0, 182501.0));
        assert_eq!(result.to_string(), "982182_00");
    }

    #[test]
    fn from_contained_point_center() {
        let tile = TileId::parse("990200_23").unwrap();
        let sw = tile.get_sw_corner();
        let center = NYSCoords2::new(*sw.easting() + 250.0, *sw.northing() + 250.0);
        let result = TileId::from_contained_point(&center);
        assert_eq!(result.to_string(), "990200_23");
    }

    #[test]
    fn from_contained_point_near_ne_corner() {
        // A point 0.1 usft inside the NE corner is still in the same tile
        let tile = TileId::parse("990200_23").unwrap();
        let sw = tile.get_sw_corner();
        let near_ne = NYSCoords2::new(*sw.easting() + 499.9, *sw.northing() + 499.9);
        let result = TileId::from_contained_point(&near_ne);
        assert_eq!(result.to_string(), "990200_23");
    }

    #[test]
    fn from_contained_point_east_boundary_crosses_to_next_subgrid() {
        // E boundary of subgrid 23 is also the SW easting of subgrid 33
        let sw_23 = TileId::parse("990200_23").unwrap().get_sw_corner();
        let on_east = NYSCoords2::new(*sw_23.easting() + 500.0, *sw_23.northing() + 250.0);
        let result = TileId::from_contained_point(&on_east);
        assert_eq!(result.to_string(), "990200_33");
    }

    #[test]
    fn from_contained_point_north_boundary_crosses_to_next_subgrid() {
        // N boundary of subgrid 23 is also the SW northing of subgrid 24
        let sw_23 = TileId::parse("990200_23").unwrap().get_sw_corner();
        let on_north = NYSCoords2::new(*sw_23.easting() + 250.0, *sw_23.northing() + 500.0);
        let result = TileId::from_contained_point(&on_north);
        assert_eq!(result.to_string(), "990200_24");
    }

    #[test]
    fn from_contained_point_crosses_las_boundary() {
        let sw_44 = TileId::parse("990200_44").unwrap().get_sw_corner();
        let on_las_ne = NYSCoords2::new(*sw_44.easting() + 500.0, *sw_44.northing() + 250.0);
        let result = TileId::from_contained_point(&on_las_ne);
        assert_eq!(result.to_string(), "992200_04");
    }

    #[test]
    fn from_contained_point_mod7_tile() {
        // Tile with components ending in 7 (component_to_usft adds 500)
        let tile = TileId::parse("987177_00").unwrap();
        let result = TileId::from_contained_point(&tile.get_sw_corner());
        assert_eq!(result.to_string(), "987177_00");
    }

    #[test]
    fn from_contained_point_mod2_tile() {
        let tile = TileId::parse("987172_24").unwrap();
        let result = TileId::from_contained_point(&tile.get_sw_corner());
        assert_eq!(result.to_string(), "987172_24");
    }

    #[test]
    fn from_contained_point_roundtrip_all_subgrids() {
        // SW corner of every subgrid within a LAS tile maps back to that tile
        for x in 0..5u8 {
            for y in 0..5u8 {
                let id = format!("990200_{x}{y}");
                let tile = TileId::parse(&id).unwrap();
                let result = TileId::from_contained_point(&tile.get_sw_corner());
                assert_eq!(result.to_string(), id, "failed for subgrid {x}{y}");
            }
        }
    }

    #[test]
    fn from_adjacent_tile_identity() {
        let tile = TileId::parse("982182_23").unwrap();
        let new_tile = TileId::from_adjacent_tile(&tile, (0, 0));
        assert_eq!(new_tile.to_string(), "982182_23");
    }

    #[test]
    fn from_adjacent_tile_negative() {
        let tile = TileId::parse("982182_23").unwrap();
        let new_tile = TileId::from_adjacent_tile(&tile, (-1, -1));
        assert_eq!(new_tile.to_string(), "982182_12");
    }

    #[test]
    fn from_adjacent_tile_positive() {
        let tile = TileId::parse("982182_23").unwrap();
        let new_tile = TileId::from_adjacent_tile(&tile, (1, 1));
        assert_eq!(new_tile.to_string(), "982182_34");
    }

    #[test]
    fn from_adjacent_tile_over_tile_border_positive() {
        let tile = TileId::parse("982182_23").unwrap();
        let new_tile = TileId::from_adjacent_tile(&tile, (3, 3));
        assert_eq!(new_tile.to_string(), "985185_01");
    }

    #[test]
    fn from_adjacent_tile_over_tile_border_negative() {
        let tile = TileId::parse("982182_23").unwrap();
        let new_tile = TileId::from_adjacent_tile(&tile, (-4, 0));
        assert_eq!(new_tile.to_string(), "980182_33");
    }

    #[test]
    fn from_adjacent_tile_over_multi_border_negative() {
        let tile = TileId::parse("982182_23").unwrap();
        let new_tile = TileId::from_adjacent_tile(&tile, (-10, 0));
        assert_eq!(new_tile.to_string(), "977182_23");
    }

    // --- TileId serde ---

    #[test]
    fn tile_id_serializes_to_string() {
        use rocket::serde::json::serde_json;
        let id = TileId::parse("500300_23").unwrap();
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"500300_23\"");
    }

    #[test]
    fn tile_id_deserializes_from_string() {
        use rocket::serde::json::serde_json;
        let id: TileId = serde_json::from_str("\"500300_23\"").unwrap();
        assert_eq!(id.to_string(), "500300_23");
    }

    #[test]
    fn tile_id_serde_roundtrip() {
        use rocket::serde::json::serde_json;
        let original = TileId::parse("987177_42").unwrap();
        let restored: TileId =
            serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn tile_id_deserialize_invalid() {
        use rocket::serde::json::serde_json;
        assert!(serde_json::from_str::<TileId>("\"invalid\"").is_err());
        assert!(serde_json::from_str::<TileId>("\"500300\"").is_err());
        assert!(serde_json::from_str::<TileId>("\"500301_23\"").is_err());
    }

    // --- TileId::as_packed_u64 ---

    #[test]
    fn packed_u64_components_round_trip() {
        // Verify each field lands in the correct bit range.
        let id = TileId::parse("500300_23").unwrap();
        let packed = id.as_packed_u64();
        assert_eq!((packed & 0xFFFF) as u16, id.las_tile_id.0);           // bits 0-15: las easting
        assert_eq!(((packed >> 16) & 0xFFFF) as u16, id.las_tile_id.1);   // bits 16-31: las northing
        assert_eq!(((packed >> 32) & 0xFF) as u8, id.subgrid_id.0);       // bits 32-39: subgrid x
        assert_eq!(((packed >> 40) & 0xFF) as u8, id.subgrid_id.1);       // bits 40-47: subgrid y
    }

    #[test]
    fn packed_u64_distinct_tiles_produce_distinct_values() {
        let ids = ["500300_00", "500300_23", "500300_44", "987177_42", "235_00"];
        let packed: Vec<u64> = ids.iter()
            .map(|s| TileId::parse(s).unwrap().as_packed_u64())
            .collect();
        // All values must be unique.
        let mut sorted = packed.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), packed.len());
    }

    #[test]
    fn packed_u64_is_stable_across_parse_roundtrip() {
        // Parsing the same string twice must produce the same packed value.
        let a = TileId::parse("987177_42").unwrap().as_packed_u64();
        let b = TileId::parse("987177_42").unwrap().as_packed_u64();
        assert_eq!(a, b);
    }

    #[test]
    fn pre_baked_nyc_tile_list_is_accurate() {
        // Mesh room (2023)
        assert!(TileId::parse("987200_10").unwrap().is_in_nyc());

        // Mesh room (2025)
        assert!(TileId::parse("987200_12").unwrap().is_in_nyc());

        // Mesh room (2026)
        assert!(TileId::parse("987195_20").unwrap().is_in_nyc());

        // Jersey City
        assert!(!TileId::parse("972200_33").unwrap().is_in_nyc());

        // Hudson river, one tile in
        assert!(TileId::parse("980215_10").unwrap().is_in_nyc());

        // Hudson river, one tile out
        assert!(!TileId::parse("972200_00").unwrap().is_in_nyc());

        // Brooklyn bridge, between LAS tiles
        assert!(TileId::parse("985195_03").unwrap().is_in_nyc());

        // Out in the bay
        assert!(TileId::parse("975137_23").unwrap().is_in_nyc());

        // Out in the bay (jersey side)
        assert!(!TileId::parse("990117_12").unwrap().is_in_nyc());

        // Great neck
        assert!(!TileId::parse("602227_12").unwrap().is_in_nyc());

        // Yonkers border (in)
        assert!(TileId::parse("10270_03").unwrap().is_in_nyc());

        // Yonkers border (out)
        assert!(!TileId::parse("10270_04").unwrap().is_in_nyc());
    }
}
