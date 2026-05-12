use rocket::http::Status;
use rocket::serde::json::Json;
use crate::analysis::point_evaluation::{evaluate_and_store, valid_analysis_frequency, PointEvaluationInput, PointEvaluationOutput};

#[post("/analyzePointPair", format = "json", data = "<point_pair>")]
pub async fn point_analysis(
    point_pair: Json<PointEvaluationInput>
) -> Result<Json<PointEvaluationOutput>, Status> {
    if !point_pair.point_a().valid() || !point_pair.point_b().valid() {
        return Err(Status::BadRequest);
    }
    if valid_analysis_frequency(*point_pair.frequency_hz()) {
        return Err(Status::BadRequest);
    }

    let result = evaluate_and_store(point_pair.into_inner());
    Ok(Json(result.into_output()))
}