mod types;
pub mod providers;
pub mod util;
pub mod endpoints;
pub mod building;

#[macro_use] extern crate rocket;

use crate::endpoints::coords::gps_to_nys;
use crate::endpoints::rooftop::render_rooftop;
use crate::endpoints::tileview::get_terrain_ortho;
use crate::providers::S3BackedProviders;
use crate::util::coord_conversion::{init_coord_converter_factory, with_coord_converter, CoordinateConverter};

#[get("/healthCheck")]
fn health_check() -> &'static str {
    "Healthy"
}

#[launch]
async fn rocket() -> _ {
    init_coord_converter_factory(|| CoordinateConverter::new());
    rocket::build()
        .manage(S3BackedProviders::new_with_s3_from_env().await)
        .mount("/api", routes![health_check])
        .mount("/api/rooftop", routes![render_rooftop])
        .mount("/api/tileview", routes![get_terrain_ortho])
        .mount("/api/coords", routes![gps_to_nys])
}
