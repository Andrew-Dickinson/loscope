use fresnel_2::endpoints::analysis::{map_overview, point_analysis};
use fresnel_2::endpoints::coords::gps_to_nys;
use fresnel_2::endpoints::rooftop::{render_rooftop, sample_points as sample_points_endpoint};
use fresnel_2::endpoints::tileview::get_terrain_ortho;
use fresnel_2::providers::Providers;
use fresnel_2::util::coord_conversion::{init_coord_converter_factory, CoordinateConverter};

#[rocket::get("/healthcheck")]
fn health_check() -> &'static str {
    "Healthy"
}

#[rocket::launch]
async fn rocket() -> _ {
    init_coord_converter_factory(CoordinateConverter::new);
    rocket::build()
        .manage(Providers::new_from_env().await.unwrap())
        .mount("/api", rocket::routes![health_check])
        .mount("/api/rooftop", rocket::routes![render_rooftop, sample_points_endpoint])
        .mount("/api/tileview", rocket::routes![get_terrain_ortho])
        .mount("/api/coords", rocket::routes![gps_to_nys])
        .mount("/api/analysis", rocket::routes![point_analysis,map_overview])
}
