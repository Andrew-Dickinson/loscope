use rocket::serde::json::Json;
use rocket::http::Status;
use rocket::serde::Deserialize;
use serde::Serialize;
use crate::types::coords::{GPSCoords3, NYSCoords3};
use crate::util::coord_conversion::with_coord_converter;

#[derive(Deserialize,Serialize)]
struct NYSCoords3Wrapped {
    nys: NYSCoords3
}

#[derive(Deserialize,Serialize)]
struct GPS3Wrapped {
    gps: GPSCoords3
}

#[post("/toNys", format = "json", data = "<gps_coords>")]
pub async fn gps_to_nys(
    gps_coords: Json<GPS3Wrapped>
) -> Result<Json<NYSCoords3Wrapped>, Status> {
    Ok(
        Json::from(
            // TODO: Is there a safety hazard due to the unbounded nature of the inputs here?
            NYSCoords3Wrapped {
                nys: with_coord_converter(
                    |converter| converter.to_nys_plane3(&gps_coords.gps)
                )
            }
        )
    )
}