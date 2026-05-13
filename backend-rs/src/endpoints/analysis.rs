use rocket::http::Status;
use rocket::serde::json::Json;
use rocket::State;
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
    if valid_analysis_frequency(*point_pair.frequency_hz()) {
        return Err(Status::BadRequest);
    }

    let result = evaluate_points(point_pair.into_inner());
    providers.point_eval_result_provider().put(&result)
        .or_else(|err| Err(Status::InternalServerError))?;

    Ok(Json(result.into_output()))
}