use crate::sample_points::point::EncodedPoint;
use crate::types::tuple_serde::{
    CoordsVisitor2, CoordsVisitor3, serialize_tuple2, serialize_tuple3,
};
use approx_derive::AbsDiffEq;
use derive_getters::Getters;
use derive_new::new;
use geo::Point;
use rocket::serde::{Deserializer, Serializer};
use serde::{Deserialize, Serialize};
use wincode::{SchemaRead, SchemaWrite};

pub const MIN_NYS_COORD_VALUE: f64 = 0.0;
pub const MAX_NYS_COORD_VALUE: f64 = 2_000_000.0;

pub const MIN_ALT_COORD_VALUE: f64 = -5000.0;
pub const MAX_ALT_COORD_VALUE: f64 = 5000.0;

#[derive(Debug, Getters, new, PartialEq, AbsDiffEq)]
pub struct GPSCoords3 {
    lat: f64,
    lon: f64,
    alt_m: f64,
}

#[derive(Debug, Getters, new, PartialEq, AbsDiffEq)]
pub struct GPSCoords2 {
    lat: f64,
    lon: f64,
}

#[derive(Debug, Getters, new, PartialEq, AbsDiffEq, Clone, SchemaWrite, SchemaRead)]
pub struct NYSCoords3 {
    easting: f64,
    northing: f64,
    alt_usft: f64,
}

#[derive(Debug, Getters, new, PartialEq, AbsDiffEq, Clone, SchemaWrite, SchemaRead)]
pub struct NYSCoords2 {
    easting: f64,
    northing: f64,
}

impl Serialize for NYSCoords3 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_tuple3(self.into(), serializer)
    }
}

impl Serialize for NYSCoords2 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_tuple2(self.into(), serializer)
    }
}

impl<'de> Deserialize<'de> for NYSCoords3 {
    fn deserialize<D: Deserializer<'de>>(des: D) -> Result<Self, D::Error> {
        Ok(des.deserialize_tuple(3, CoordsVisitor3)?.into())
    }
}

impl<'de> Deserialize<'de> for NYSCoords2 {
    fn deserialize<D: Deserializer<'de>>(des: D) -> Result<Self, D::Error> {
        Ok(des.deserialize_tuple(3, CoordsVisitor2)?.into())
    }
}

impl Serialize for GPSCoords3 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_tuple3(self.into(), serializer)
    }
}

impl Serialize for GPSCoords2 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_tuple2(self.into(), serializer)
    }
}

impl<'de> Deserialize<'de> for GPSCoords3 {
    fn deserialize<D: Deserializer<'de>>(des: D) -> Result<Self, D::Error> {
        Ok(des.deserialize_tuple(3, CoordsVisitor3)?.into())
    }
}

impl<'de> Deserialize<'de> for GPSCoords2 {
    fn deserialize<D: Deserializer<'de>>(des: D) -> Result<Self, D::Error> {
        Ok(des.deserialize_tuple(3, CoordsVisitor2)?.into())
    }
}

#[derive(Debug, Getters, new, PartialEq, AbsDiffEq)]
pub struct RelativeCoords3 {
    x: f64,
    y: f64,
    alt_usft: f64,
}

impl<'de> Deserialize<'de> for RelativeCoords3 {
    fn deserialize<D: Deserializer<'de>>(des: D) -> Result<Self, D::Error> {
        Ok(des.deserialize_tuple(3, CoordsVisitor3)?.into())
    }
}

impl Serialize for RelativeCoords3 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_tuple3(self.into(), serializer)
    }
}

impl NYSCoords2 {
    pub fn from3(coords: &NYSCoords3) -> NYSCoords2 {
        NYSCoords2 {
            easting: coords.easting,
            northing: coords.northing,
        }
    }

    pub fn valid(&self) -> bool {
        valid_nys_coordinate(self.easting) && valid_nys_coordinate(self.northing)
    }
}

impl NYSCoords3 {
    pub fn from2(coords: &NYSCoords2, alt: f64) -> NYSCoords3 {
        NYSCoords3 {
            easting: coords.easting,
            northing: coords.northing,
            alt_usft: alt,
        }
    }

    pub fn valid(&self) -> bool {
        valid_nys_coordinate(self.easting)
            && valid_nys_coordinate(self.northing)
            && valid_alt_coordinate(self.alt_usft)
    }

    pub fn relative_from_base(&self, sw_offset: &NYSCoords3) -> RelativeCoords3 {
        RelativeCoords3::new(
            self.easting - sw_offset.easting,
            self.northing - sw_offset.northing,
            self.alt_usft - sw_offset.alt_usft,
        )
    }

    pub fn encoded_from_base(&self, sw_offset: &NYSCoords3) -> EncodedPoint {
        EncodedPoint::new(self.relative_from_base(sw_offset), self.clone())
    }
}

impl GPSCoords2 {
    pub fn from3(coords: &GPSCoords3) -> GPSCoords2 {
        GPSCoords2 {
            lat: coords.lat,
            lon: coords.lon,
        }
    }
}

impl GPSCoords3 {
    pub fn from2(coords: &GPSCoords2, alt: f64) -> GPSCoords3 {
        GPSCoords3 {
            lat: coords.lat,
            lon: coords.lon,
            alt_m: alt,
        }
    }
}

impl From<&NYSCoords3> for NYSCoords2 {
    fn from(item: &NYSCoords3) -> Self {
        NYSCoords2::from3(item)
    }
}

impl From<&NYSCoords2> for Point {
    fn from(item: &NYSCoords2) -> Self {
        Point::new(item.easting, item.northing)
    }
}

impl From<&NYSCoords3> for Point {
    fn from(item: &NYSCoords3) -> Self {
        Point::new(item.easting, item.northing)
    }
}

impl From<&NYSCoords3> for (f64, f64, f64) {
    fn from(item: &NYSCoords3) -> Self {
        (item.easting, item.northing, item.alt_usft)
    }
}

impl From<&NYSCoords2> for (f64, f64) {
    fn from(item: &NYSCoords2) -> Self {
        (item.easting, item.northing)
    }
}

impl From<(f64, f64, f64)> for NYSCoords3 {
    fn from(item: (f64, f64, f64)) -> Self {
        NYSCoords3::new(item.0, item.1, item.2)
    }
}

impl From<(f64, f64)> for NYSCoords2 {
    fn from(item: (f64, f64)) -> Self {
        NYSCoords2::new(item.0, item.1)
    }
}

impl From<&GPSCoords3> for (f64, f64, f64) {
    fn from(item: &GPSCoords3) -> Self {
        (item.lat, item.lon, item.alt_m)
    }
}

impl From<&GPSCoords2> for (f64, f64) {
    fn from(item: &GPSCoords2) -> Self {
        (item.lat, item.lon)
    }
}

impl From<(f64, f64, f64)> for GPSCoords3 {
    fn from(item: (f64, f64, f64)) -> Self {
        GPSCoords3::new(item.0, item.1, item.2)
    }
}

impl From<(f64, f64)> for GPSCoords2 {
    fn from(item: (f64, f64)) -> Self {
        GPSCoords2::new(item.0, item.1)
    }
}

impl From<(f64, f64, f64)> for RelativeCoords3 {
    fn from(item: (f64, f64, f64)) -> Self {
        RelativeCoords3::new(item.0, item.1, item.2)
    }
}

impl From<&RelativeCoords3> for (f64, f64, f64) {
    fn from(item: &RelativeCoords3) -> Self {
        (item.x, item.y, item.alt_usft)
    }
}

pub fn valid_nys_coordinate(coord: f64) -> bool {
    (MIN_NYS_COORD_VALUE..=MAX_NYS_COORD_VALUE).contains(&coord)
}

pub fn valid_alt_coordinate(coord: f64) -> bool {
    (MIN_ALT_COORD_VALUE..=MAX_ALT_COORD_VALUE).contains(&coord)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nys2_from3_drops_altitude() {
        let c3 = NYSCoords3::new(1000.0, 2000.0, 50.0);
        let c2 = NYSCoords2::from3(&c3);
        assert_eq!(*c2.easting(), 1000.0);
        assert_eq!(*c2.northing(), 2000.0);
    }

    #[test]
    fn nys3_from2_sets_altitude() {
        let c2 = NYSCoords2::new(1000.0, 2000.0);
        let c3 = NYSCoords3::from2(&c2, 75.5);
        assert_eq!(*c3.easting(), 1000.0);
        assert_eq!(*c3.northing(), 2000.0);
        assert_eq!(*c3.alt_usft(), 75.5);
    }

    #[test]
    fn nys_roundtrip_via_2() {
        let original = NYSCoords3::new(123.4, 567.8, 99.9);
        let c2 = NYSCoords2::from3(&original);
        let restored = NYSCoords3::from2(&c2, *original.alt_usft());
        assert_eq!(original, restored);
    }

    #[test]
    fn gps2_from3_drops_altitude() {
        let c3 = GPSCoords3::new(40.7128, -74.0060, 10.0);
        let c2 = GPSCoords2::from3(&c3);
        assert_eq!(*c2.lat(), 40.7128);
        assert_eq!(*c2.lon(), -74.0060);
    }

    #[test]
    fn gps3_from2_sets_altitude() {
        let c2 = GPSCoords2::new(40.7128, -74.0060);
        let c3 = GPSCoords3::from2(&c2, 15.0);
        assert_eq!(*c3.lat(), 40.7128);
        assert_eq!(*c3.lon(), -74.0060);
        assert_eq!(*c3.alt_m(), 15.0);
    }

    #[test]
    fn gps_roundtrip_via_2() {
        let original = GPSCoords3::new(40.7128, -74.0060, 22.5);
        let c2 = GPSCoords2::from3(&original);
        let restored = GPSCoords3::from2(&c2, *original.alt_m());
        assert_eq!(original, restored);
    }

    #[test]
    fn nys3_from2_zero_altitude() {
        let c2 = NYSCoords2::new(500.0, 1500.0);
        let c3 = NYSCoords3::from2(&c2, 0.0);
        assert_eq!(*c3.alt_usft(), 0.0);
    }

    #[test]
    fn gps3_from2_zero_altitude() {
        let c2 = GPSCoords2::new(34.0, -118.0);
        let c3 = GPSCoords3::from2(&c2, 0.0);
        assert_eq!(*c3.alt_m(), 0.0);
    }

    // --- valid_nys_coordinate ---

    #[test]
    fn valid_nys_coordinate_min_boundary() {
        assert!(valid_nys_coordinate(MIN_NYS_COORD_VALUE));
    }

    #[test]
    fn valid_nys_coordinate_max_boundary() {
        assert!(valid_nys_coordinate(MAX_NYS_COORD_VALUE));
    }

    #[test]
    fn valid_nys_coordinate_interior() {
        assert!(valid_nys_coordinate(1_000_000.0));
        assert!(valid_nys_coordinate(500_000.0));
    }

    #[test]
    fn valid_nys_coordinate_below_min() {
        assert!(!valid_nys_coordinate(MIN_NYS_COORD_VALUE - 1.0));
        assert!(!valid_nys_coordinate(-1.0));
    }

    #[test]
    fn valid_nys_coordinate_above_max() {
        assert!(!valid_nys_coordinate(MAX_NYS_COORD_VALUE + 1.0));
        assert!(!valid_nys_coordinate(3_000_000.0));
    }
}
