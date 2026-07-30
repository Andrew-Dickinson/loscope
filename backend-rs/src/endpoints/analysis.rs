use crate::analysis::fresnel_kml::build_fresnel_kml;
use crate::analysis::fresnel_zone_obj::stream_fresnel_tile_slice_as_obj;
use crate::analysis::intersection_vis::tile_intersection_to_png;
use crate::analysis::map_overview::PointEvaluationOverview;
use crate::analysis::memory_budget::MemoryBudget;
use crate::analysis::memory_estimate::{
    estimate_analysis_bytes_precise, estimate_analysis_result_bytes, estimate_full_recompute_bytes,
    intersection_visualization_png_bytes,
};
use crate::analysis::point_evaluation::{PointEvaluationInput, PointEvaluationOutput, evaluate_points, valid_analysis_frequency, PointEvaluationOutcome};
use crate::endpoints::api_error::ApiError;
use crate::providers::Providers;
use crate::types::tiles::TileId;
use crate::util::coord_conversion::with_coord_converter;
use futures_util::StreamExt;
use kml::Kml;
use rocket::http::{ContentType, Header, Status};
use rocket::response::Responder;
use rocket::response::stream::TextStream;
use rocket::serde::json::Json;
use rocket::State;
use uuid::Uuid;

#[derive(Responder)]
#[response(status = 200, content_type = "image/png")]
pub struct PngImage(Vec<u8>);

#[derive(Responder)]
#[response(status = 200, content_type = "application/vnd.google-earth.kml+xml")]
pub struct KmlDownload {
    content: String,
    disposition: Header<'static>,
}

impl KmlDownload {
    pub fn new(kml: Kml, analysis_id: &Uuid) -> KmlDownload {
        Self {
            content: kml.to_string(),
            disposition: Header::new(
                "Content-Disposition",
                format!("attachment; filename=\"fresnel-zone-{}.kml\"", analysis_id),
            ),
        }
    }
}

#[post("/analyzePointPair", format = "json", data = "<point_pair>")]
pub async fn point_analysis(
    point_pair: Json<PointEvaluationInput>,
    providers: &State<Providers>,
    memory_budget: &State<MemoryBudget>,
) -> Result<Json<PointEvaluationOutput>, ApiError> {
    if !point_pair.point_a().valid() || !point_pair.point_b().valid() {
        return Err(Status::BadRequest.into());
    }
    if !valid_analysis_frequency(*point_pair.frequency_hz()) {
        return Err(Status::BadRequest.into());
    }

    let input = point_pair.into_inner();

    // Reserve our estimated share of the process memory budget up front, before doing any of the
    // actual (potentially huge) allocation work. Uses the real per-tile obstruction count from
    // the (cheap, in-memory) obstruction index rather than a flat per-tile guess, so this is
    // accurate rather than a worst-case-for-every-tile padding figure.
    let estimate =
        estimate_analysis_bytes_precise(&input, providers.obstruction_provider().as_ref()).await?;
    let mut reservation = memory_budget.try_reserve(estimate)?;
    // Computed now (cheap, geometry-only) so it's ready the moment evaluate_points returns —
    // see the shrink_to() call below.
    let result_size_estimate = estimate_analysis_result_bytes(&input);

    // TODO: Detect vegetation-only obstructions and report them as seasonal

    let result = evaluate_points(
        Uuid::new_v4(),
        input,
        providers.elevation_tile_provider().as_ref(),
        providers.obstruction_provider().as_ref(),
    )
    .await
    .map_err(|err| {
        eprintln!("{:?}", err);
        err
    })?;

    // The terrain grids and obstruction rasters evaluate_points needed to compute this are
    // already dropped by the time it returns — shrink the reservation to match what's actually
    // still resident (the zone/intersection arrays in `result`) so other concurrent requests see
    // this headroom freed immediately, rather than waiting for this handler to finish entirely
    // (which still holds the reservation, now much smaller, until it returns).
    reservation.shrink_to(result_size_estimate);

    // We do a slightly unexpected thing here. We intentionally throw away a chunk of the analysis
    // results to reduce the size of artifacts stored on the backend. This is because the vast
    // majority of analyses are never investigated beyond their overall status (Green/Yellow/Red).
    // For the few that are, we will lazily recompute the large chunks of the result on the fly
    // after the first request that requires it
    let result_envelope = PointEvaluationOutcome::Lite(result.into());

    providers
        .point_eval_result_provider()
        .put(&result_envelope)
        .map_err(|err| {
            eprintln!("{:?}", err);
            err
        }).map_err(|_| ApiError::new(Status::InternalServerError))?;

    Ok(Json(result_envelope.into()))
}

/// Shared by the four endpoints below: peeks at the stored (Lite or Full) outcome's input via a
/// cheap `get()` (no recompute) so it's known before committing to a reservation, then computes
/// (a) the peak-bytes estimate for the `get_full` recompute the caller is about to trigger and
/// (b) the much smaller size it can `shrink_to()` once `get_full` actually returns (see
/// `estimate_analysis_result_bytes`'s doc comment for why that's safe). Returns estimates, not an
/// already-made `Reservation`, so callers that need to reserve additional headroom on top (e.g.
/// `intersection_visualization`, below) can fold it into a single `try_reserve` call rather than
/// needing to grow an existing reservation (which `Reservation` deliberately doesn't support).
///
/// `get_full` re-reads the outcome itself (its own `get()` call) rather than being handed the one
/// fetched here — a small redundant read against a cheap KV lookup, traded for keeping this
/// estimation logic decoupled from `get_full`'s internals.
async fn estimate_for_get_full(
    analysis_id: &Uuid,
    providers: &Providers,
) -> Result<(u64, u64), ApiError> {
    let outcome = providers.point_eval_result_provider().get(analysis_id)?;
    let input = outcome.output().input();
    let peak_estimate =
        estimate_full_recompute_bytes(input, providers.obstruction_provider().as_ref()).await?;
    let result_size_estimate = estimate_analysis_result_bytes(input);
    Ok((peak_estimate, result_size_estimate))
}

#[post("/overview/<analysis_id>")]
pub async fn map_overview(
    analysis_id: &str,
    providers: &State<Providers>,
    memory_budget: &State<MemoryBudget>,
) -> Result<Json<PointEvaluationOverview>, ApiError> {
    let analysis_id = Uuid::parse_str(analysis_id).map_err(|_| Status::BadRequest)?;

    let (peak_estimate, result_size_estimate) =
        estimate_for_get_full(&analysis_id, providers).await?;
    let mut reservation = memory_budget.try_reserve(peak_estimate)?;

    let analysis_outcome = providers.point_eval_result_provider().get_full(
        &analysis_id,
        providers.elevation_tile_provider().as_ref(),
        providers.obstruction_provider().as_ref()
    ).await?;
    reservation.shrink_to(result_size_estimate);

    Ok(Json((&analysis_outcome).into()))
}

#[post("/intersectionVisualization/<analysis_id>/<tile_id>")]
pub async fn intersection_visualization(
    analysis_id: &str,
    tile_id: &str,
    providers: &State<Providers>,
    memory_budget: &State<MemoryBudget>,
) -> Result<PngImage, ApiError> {
    let analysis_id = Uuid::parse_str(analysis_id).map_err(|_| Status::BadRequest)?;
    let Ok(tile_id) = TileId::parse(tile_id) else {
        return Err(Status::BadRequest.into());
    };

    // The PNG rendering below reads from the get_full result, so both need to be covered by one
    // reservation at once — fold the (fixed-size, see intersection_visualization_png_bytes' doc
    // comment) PNG allowance into the same try_reserve call as the recompute peak, rather than
    // shrinking in between. The reservation is dropped when this function returns either way, so
    // there's no separate benefit to shrinking again after rendering finishes.
    let (peak_estimate, _) = estimate_for_get_full(&analysis_id, providers).await?;
    let _reservation =
        memory_budget.try_reserve(peak_estimate + intersection_visualization_png_bytes())?;

    let analysis_outcome = providers.point_eval_result_provider().get_full(
        &analysis_id,
        providers.elevation_tile_provider().as_ref(),
        providers.obstruction_provider().as_ref()
    ).await?;

    let Some(png_bytes) = tile_intersection_to_png(
        analysis_outcome
            .result_full()
            .intersection()
            .rasterize_in_tile(tile_id),
    ) else {
        return Err(Status::NoContent.into());
    };

    Ok(PngImage(png_bytes))
}

#[post("/fresnelSliceObj/<analysis_id>/<tile_id>")]
pub async fn get_fresnel_slice_obj(
    analysis_id: &str,
    tile_id: &str,
    providers: &State<Providers>,
    memory_budget: &State<MemoryBudget>,
) -> Result<(ContentType, TextStream![String]), ApiError> {
    let analysis_id = Uuid::parse_str(analysis_id).map_err(|_| Status::BadRequest)?;
    let Ok(tile_id) = TileId::parse(tile_id) else {
        return Err(Status::BadRequest.into());
    };

    let (peak_estimate, result_size_estimate) =
        estimate_for_get_full(&analysis_id, providers).await?;
    let mut reservation = memory_budget.try_reserve(peak_estimate)?;

    let analysis = providers.point_eval_result_provider().get_full(
        &analysis_id,
        providers.elevation_tile_provider().as_ref(),
        providers.obstruction_provider().as_ref()
    ).await?;
    // Unlike the non-streaming endpoints, the zone data this references has to stay resident for
    // as long as the client is still downloading the stream, not just until this function
    // returns — so the (now-shrunk) reservation is moved into the stream body rather than
    // released here. Same pattern as render_rooftop/get_terrain_obstruction_obj.
    reservation.shrink_to(result_size_estimate);

    let obj_stream = TextStream! {
        let _reservation = reservation;
        let mut stream = std::pin::pin!(
            stream_fresnel_tile_slice_as_obj(analysis_id, analysis.result_full().zone(), tile_id)
        );
        while let Some(chunk) = stream.next().await {
            yield chunk;
        }
    };

    Ok((ContentType::new("model", "obj"), obj_stream))
}

#[post("/fresnelKml/<analysis_id>")]
pub async fn fresnel_kml(
    analysis_id: &str,
    providers: &State<Providers>,
    memory_budget: &State<MemoryBudget>,
) -> Result<KmlDownload, ApiError> {
    let analysis_id = Uuid::parse_str(analysis_id).map_err(|_| Status::BadRequest)?;

    let (peak_estimate, result_size_estimate) =
        estimate_for_get_full(&analysis_id, providers).await?;
    let mut reservation = memory_budget.try_reserve(peak_estimate)?;

    let analysis_outcome = providers.point_eval_result_provider().get_full(
        &analysis_id,
        providers.elevation_tile_provider().as_ref(),
        providers.obstruction_provider().as_ref()
    ).await?;
    reservation.shrink_to(result_size_estimate);

    let original_inputs = analysis_outcome.output().input();

    let kml = with_coord_converter(|converter| {
        build_fresnel_kml(
            &analysis_id,
            converter.to_gps3(original_inputs.point_a()),
            converter.to_gps3(original_inputs.point_b()),
            *original_inputs.frequency_hz(),
        )
    });

    Ok(KmlDownload::new(kml, &analysis_id))
}
