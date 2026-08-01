use loscope::analysis::memory_budget::MemoryBudget;
use loscope::endpoints::analysis::{
    fresnel_kml, get_fresnel_slice_obj, intersection_visualization, map_overview, point_analysis,
};
use loscope::endpoints::coords::gps_to_nys;
use loscope::endpoints::meshdb::resolve_number;
use loscope::endpoints::rooftop::{
    background_tile_ids, background_tile_raster, render_rooftop,
    sample_points as sample_points_endpoint,
};
use loscope::endpoints::tileview::{
    get_terrain_obstruction_meta, get_terrain_obstruction_obj, get_terrain_ortho,
    get_terrain_raster, get_terrain_tile_overview,
};
use loscope::providers::Providers;
use loscope::util::coord_conversion::{CoordinateConverter, init_coord_converter_factory};
use loscope::util::download_concurrency_profiler;
use loscope::util::memory_profiler;

#[rocket::get("/healthcheck")]
fn health_check() -> &'static str {
    "Healthy"
}

#[rocket::launch]
async fn rocket() -> _ {
    init_coord_converter_factory(CoordinateConverter::new);
    let memory_budget = MemoryBudget::new_from_env();
    memory_profiler::start_if_configured(memory_budget.clone());
    download_concurrency_profiler::start_if_configured();
    rocket::build()
        .manage(Providers::new_from_env().await.unwrap())
        .manage(memory_budget)
        .mount("/api", rocket::routes![health_check])
        .mount(
            "/api/rooftop",
            rocket::routes![
                render_rooftop,
                sample_points_endpoint,
                background_tile_ids,
                background_tile_raster
            ],
        )
        .mount(
            "/api/tileview",
            rocket::routes![
                get_terrain_tile_overview,
                get_terrain_raster,
                get_terrain_obstruction_meta,
                get_terrain_obstruction_obj,
                get_terrain_ortho
            ],
        )
        .mount("/api/coords", rocket::routes![gps_to_nys])
        .mount(
            "/api/analysis",
            rocket::routes![
                point_analysis,
                map_overview,
                intersection_visualization,
                fresnel_kml,
                get_fresnel_slice_obj
            ],
        )
        .mount("/api/meshdb", rocket::routes![resolve_number])
}
