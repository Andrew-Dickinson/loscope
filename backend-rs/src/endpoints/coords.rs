use rocket::serde::json::Json;
use rocket::http::Status;
use rocket::serde::Deserialize;
use serde::Serialize;
use crate::types::coords::{GPSCoords2, GPSCoords3, NYSCoords3};
use crate::util::coord_conversion::{with_coord_converter, CoordinateConverter};

#[derive(Deserialize,Serialize)]
pub struct NYSCoords3Wrapped {
    nys: NYSCoords3
}

#[derive(Deserialize,Serialize)]
pub struct GPS3Wrapped {
    gps: GPSCoords3
}

#[post("/toNys", format = "json", data = "<gps_coords>")]
pub async fn gps_to_nys(
    gps_coords: Json<GPS3Wrapped>
) -> Result<Json<NYSCoords3Wrapped>, Status> {
    if !CoordinateConverter::valid_for_conversion(&GPSCoords2::from3(&gps_coords.gps)) {
        return Err(Status::BadRequest);
    }
    Ok(
        Json::from(
            NYSCoords3Wrapped {
                nys: with_coord_converter(
                    |converter| converter.to_nys_plane3(&gps_coords.gps)
                )
            }
        )
    )
}