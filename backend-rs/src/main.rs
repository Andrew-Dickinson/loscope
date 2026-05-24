use fresnel_2::endpoints::analysis::{fresnel_kml, intersection_visualization, map_overview, point_analysis};
use fresnel_2::endpoints::coords::gps_to_nys;
use fresnel_2::endpoints::meshdb::resolve_number;
use fresnel_2::endpoints::rooftop::{render_rooftop, sample_points as sample_points_endpoint};
use fresnel_2::endpoints::tileview::{get_fresnel_slice_obj, get_terrain_obstruction_meta, get_terrain_obstruction_obj, get_terrain_ortho, get_terrain_raster, get_terrain_tile_overview};
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
        .mount("/api/tileview", rocket::routes![get_terrain_tile_overview,get_terrain_raster,get_terrain_obstruction_meta,get_terrain_obstruction_obj,get_fresnel_slice_obj,get_terrain_ortho])
        .mount("/api/coords", rocket::routes![gps_to_nys])
        .mount("/api/analysis", rocket::routes![point_analysis,map_overview,intersection_visualization,fresnel_kml])
        .mount("/api/meshdb", rocket::routes![resolve_number])
}
