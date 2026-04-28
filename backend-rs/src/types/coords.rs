use std::ops::Deref;
use std::sync::Arc;
use derive_getters::Getters;
use derive_new::new;
use eproj::{Coordinate3, Projector, SpatialReferenceIdentifier};
use approx_derive::AbsDiffEq;
use serde::{Serialize,Deserialize};

#[derive(Debug, Getters, new, PartialEq, AbsDiffEq, Serialize, Deserialize)]
pub struct GPSCoords3 {
   lat: f64,
   lon: f64,
   alt_m: f64
}

#[derive(Debug, Getters, new, PartialEq, AbsDiffEq, Serialize, Deserialize)]
pub struct GPSCoords2 {
   lat: f64,
   lon: f64
}

#[derive(Debug, Getters, new, PartialEq, AbsDiffEq, Serialize, Deserialize)]
pub struct NYSCoords3 {
    #[serde(rename = "nys_x")]
    easting: f64,

    #[serde(rename = "nys_y")]
    northing: f64,

    #[serde(rename = "nys_z")]
    alt_usft: f64
}

#[derive(Debug, Getters, new, PartialEq, AbsDiffEq, Serialize, Deserialize)]
pub struct NYSCoords2 {
    #[serde(rename = "nys_x")]
    easting: f64,

    #[serde(rename = "nys_y")]
    northing: f64
}

impl NYSCoords2 {
    pub fn from3(coords: &NYSCoords3) -> NYSCoords2 {
        NYSCoords2 {
            easting: coords.easting,
            northing: coords.northing
        }
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
}

impl GPSCoords2 {
    pub fn from3(coords: &GPSCoords3) -> GPSCoords2 {
        GPSCoords2 {
            lat: coords.lat,
            lon: coords.lon
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
}