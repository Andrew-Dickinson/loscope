use rocket::http::Status;
use rocket::serde::json::Json;
use rocket::State;
use uuid::Uuid;
use crate::analysis::map_overview::PointEvaluationOverview;
use crate::analysis::point_evaluation::{evaluate_points, valid_analysis_frequency, PointEvaluationInput, PointEvaluationOutput};
use crate::providers::Providers;

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
        //.map_err(|err| { eprintln!("{:?}", err); err })
    ?;

    providers.point_eval_result_provider().put(&result)
        .or_else(|_| Err(Status::InternalServerError))?;

    Ok(Json(result.into_output()))
}

#[get("/overview/<analysis_id>")]
pub async fn map_overview(
    analysis_id: &str,
    providers: &State<Providers>
) -> Result<Json<PointEvaluationOverview>, Status> {
    let analysis_id = Uuid::parse_str(analysis_id).map_err(|_| Status::BadRequest)?;
    let analysis_outcome = providers.point_eval_result_provider().get(&analysis_id)
        // .map_err(|err| { eprintln!("{:?}", err); err })
        ?;
    Ok(Json((&analysis_outcome).into()))
}

