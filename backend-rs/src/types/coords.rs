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