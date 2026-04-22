mod types;

#[macro_use] extern crate rocket;

use rocket::http::Status;
use rocket::request::FromParam;
use crate::types::errors::ParseErr;
use crate::types::tiles::{SubgridId, TileId};

#[get("/")]
fn index() -> &'static str {
    "Hello, world!"
}


impl<'r> FromParam<'r> for TileId {
    type Error = ParseErr;

    fn from_param(param: &'r str) -> Result<Self, Self::Error> {
        TileId::parse(param)
    }
}

#[get("/api/tileview/terrain/orthoImage/<tile_id>")]
fn get_terrain_ortho(tile_id: Result<TileId, ParseErr>) -> Result<Box<String>, Status> {
    let Ok(tile_id) = tile_id else { return Err(Status::BadRequest) };
    Ok(Box::new(format!("{tile_id:?}")))
}

#[launch]
fn rocket() -> _ {
    rocket::build().mount("/", routes![index, get_terrain_ortho])
}