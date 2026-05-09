mod types;
pub mod providers;
pub mod util;
pub mod endpoints;
pub mod building;

#[macro_use] extern crate rocket;

use crate::endpoints::coords::gps_to_nys;
use crate::endpoints::rooftop::render_rooftop;
use crate::endpoints::tileview::get_terrain_ortho;
use crate::providers::Providers;
use crate::util::coord_conversion::{init_coord_converter_factory, CoordinateConverter};

#[get("/healthCheck")]
fn health_check() -> &'static str {
    "Healthy"
}

#[launch]
async fn rocket() -> _ {
    init_coord_converter_factory(CoordinateConverter::new);
    rocket::build()
        .manage(Providers::new_from_env().await.unwrap())
        .mount("/api", routes![health_check])
        .mount("/api/rooftop", routes![render_rooftop])
        .mount("/api/tileview", routes![get_terrain_ortho])
        .mount("/api/coords", routes![gps_to_nys])
}
