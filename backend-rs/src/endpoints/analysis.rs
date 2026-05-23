use std::io::Cursor;
use image::ImageFormat;
use rocket::http::Status;
use rocket::serde::json::Json;
use rocket::State;
use typed_floats::tf64::PositiveFinite;
use uuid::Uuid;
use crate::analysis::intersection_vis::tile_intersection_to_img;
use crate::analysis::map_overview::PointEvaluationOverview;
use crate::analysis::point_evaluation::{evaluate_points, valid_analysis_frequency, PointEvaluationInput, PointEvaluationOutput};
use crate::providers::Providers;
use crate::types::tiles::TileId;

#[derive(Responder)]
#[response(status = 200, content_type = "image/png")]
pub struct PngImage(Vec<u8>);

#[post("/analyzePointPair", format = "json", data = "<point_pair>")]
pub async fn point_analysis(
    point_pair: Json<PointEvaluationInput>,
    providers: &State<Providers>
) -> Result<Json<PointEvaluationOutput>, Status> {
    if !point_pair.point_a().valid() || !point_pair.point_b().valid() {
        return Err(Status::BadRequest);
    }
    if !valid_analysis_frequency(*point_pair.frequency_hz()) {
        return Err(Status::BadRequest);
    }

    let result = evaluate_points(
        point_pair.into_inner(),
        providers.elevation_tile_provider().as_ref()
    ).await
        .map_err(|err| { eprintln!("{:?}", err); err })
    ?;

    providers.point_eval_result_provider().put(&result)
        .map_err(|err| { eprintln!("{:?}", err); err })
        .or_else(|_| Err(Status::InternalServerError))?;

    Ok(Json(result.into_output()))
}

#[get("/overview/<analysis_id>")]
pub async fn map_overview(
    analysis_id: &str,
    providers: &State<Providers>
) -> Result<Json<PointEvaluationOverview>, Status> {
    let analysis_id = Uuid::parse_str(analysis_id).map_err(|_| Status::BadRequest)?;
    let analysis_outcome = providers.point_eval_result_provider().get(&analysis_id)?;
    Ok(Json((&analysis_outcome).into()))
}

#[get("/intersectionVisualization/<analysis_id>/<tile_id>")]
pub async fn intersection_visualization(
    analysis_id: &str,
    tile_id: &str,
    providers: &State<Providers>
) -> Result<PngImage, Status> {
    let analysis_id = Uuid::parse_str(analysis_id).map_err(|_| Status::BadRequest)?;
    let Ok(tile_id) = TileId::parse(&tile_id) else { return Err(Status::BadRequest) };
    let analysis_outcome = providers.point_eval_result_provider().get(&analysis_id)?;

    let Some(intersection_img) = tile_intersection_to_img(
        analysis_outcome.result_full()
            .intersection()
            .rasterize_in_tile(tile_id)
    ) else {
        return Err(Status::NoContent);
    };

    let mut png_bytes: Vec<u8> = Vec::new();
    intersection_img.write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png).unwrap();
    Ok(PngImage(png_bytes))
}

