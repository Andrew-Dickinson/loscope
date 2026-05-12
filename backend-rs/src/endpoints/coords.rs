use rocket::serde::json::Json;
use rocket::http::Status;
use crate::types::coords::{GPSCoords3, NYSCoords3};
use crate::util::coord_conversion::with_coord_converter;

#[post("/toNys", format = "json", data = "<gps_coords>")]
pub async fn gps_to_nys(
    gps_coords: Json<GPSCoords3>
) -> Result<Json<NYSCoords3>, Status> {
    Ok(
        Json::from(
            // TODO: Is there a safety hazard due to the unbounded nature of the inputs here?
            with_coord_converter(
                |converter| converter.to_nys_plane3(&gps_coords.0)
            )
        )
    )
}