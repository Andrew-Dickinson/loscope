mod types;
pub mod providers;
pub mod util;

#[macro_use] extern crate rocket;

use std::io::Cursor;
use image::{EncodableLayout, ImageFormat};
use rocket::http::{ContentType, Status};
use rocket::request::Outcome;
use rocket::State;
use crate::providers::ortho_provider::OrthoProvider;
use crate::providers::S3BackedProviders;
use crate::types::errors::{AssetErr, ParseErr};
use crate::types::tiles::TileId;
use types::coords::{GPSCoords3, NYSCoords3};
use rocket::serde::json::Json;
use crate::util::coord_conversion::{init_coord_converter_factory, with_coord_converter, CoordinateConverter};

#[get("/healthCheck")]
fn index() -> &'static str {
    "Healthy"
}


#[derive(Responder)]
#[response(status = 200, content_type = "image/jpeg")]
struct JpegImage(Vec<u8>);


#[get("/tileview/terrain/orthoImage/<tile_id>")]
async fn get_terrain_ortho(
    tile_id: &str,
    providers: &State<S3BackedProviders>
) -> Result<JpegImage, Status> {
    // TODO: Would it be better to use a CDN style direct browser file access for this?
    //   Especially since the requests will be balanced across many workers, which may or may
    //   not have shared cache storage
    let Ok(tile_id) = TileId::parse(&tile_id) else { return Err(Status::BadRequest) };

    let ortho_img = providers.ortho_provider().get_ortho(&tile_id).await
        .or_else(|err| {
            println!("{err:?}");
            Err(match err {
                AssetErr::AssetNotFound(_) => Status::NotFound,
                _ => Status::InternalServerError
            })
        })?;

    let mut jpeg_bytes: Vec<u8> = Vec::new();
    ortho_img.write_to(&mut Cursor::new(&mut jpeg_bytes), ImageFormat::Jpeg).unwrap();

    Ok(JpegImage(jpeg_bytes))
}


#[post("/coords/toNys", format = "json", data = "<gps_coords>")]
async fn gps_to_nys(
    gps_coords: Json<GPSCoords3>
) -> Result<Json<NYSCoords3>, Status> {
    Ok(
        Json::from(
            with_coord_converter(
                |converter| converter.to_nys_plane3(&gps_coords.0)
            )
        )
    )
}

#[launch]
async fn rocket() -> _ {
    init_coord_converter_factory(|| CoordinateConverter::new());
    rocket::build()
        .manage(S3BackedProviders::new_with_s3_from_env().await)
        .mount("/api", routes![index, get_terrain_ortho, gps_to_nys])
}
