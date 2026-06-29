use crate::analysis::fresnel_kml::build_fresnel_kml;
use crate::analysis::fresnel_zone_obj::stream_fresnel_tile_slice_as_obj;
use crate::analysis::intersection_vis::tile_intersection_to_png;
use crate::analysis::map_overview::PointEvaluationOverview;
use crate::analysis::point_evaluation::{PointEvaluationInput, PointEvaluationOutput, evaluate_points, valid_analysis_frequency, PointEvaluationOutcome};
use crate::providers::Providers;
use crate::types::tiles::TileId;
use crate::util::coord_conversion::with_coord_converter;
use futures_util::StreamExt;
use kml::Kml;
use rocket::http::{ContentType, Header, Status};
use rocket::response::Responder;
use rocket::response::stream::TextStream;
use rocket::serde::json::Json;
use rocket::{State};
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
) -> Result<Json<PointEvaluationOutput>, Status> {
    if !point_pair.point_a().valid() || !point_pair.point_b().valid() {
        return Err(Status::BadRequest);
    }
    if !valid_analysis_frequency(*point_pair.frequency_hz()) {
        return Err(Status::BadRequest);
    }

    // TODO: Detect vegetation-only obstructions and report them as seasonal

    let result = evaluate_points(
        Uuid::new_v4(),
        point_pair.into_inner(),
        providers.elevation_tile_provider().as_ref(),
        providers.obstruction_provider().as_ref(),
    )
    .await
    .map_err(|err| {
        eprintln!("{:?}", err);
        err
    })?;

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
        }).map_err(|_| Status::InternalServerError)?;

    Ok(Json(result_envelope.into()))
}

#[post("/overview/<analysis_id>")]
pub async fn map_overview(
    analysis_id: &str,
    providers: &State<Providers>,
) -> Result<Json<PointEvaluationOverview>, Status> {
    let analysis_id = Uuid::parse_str(analysis_id).map_err(|_| Status::BadRequest)?;

    let analysis_outcome = providers.point_eval_result_provider().get_full(
        &analysis_id,
        providers.elevation_tile_provider().as_ref(),
        providers.obstruction_provider().as_ref()
    ).await?;

    Ok(Json((&analysis_outcome).into()))
}

#[post("/intersectionVisualization/<analysis_id>/<tile_id>")]
pub async fn intersection_visualization(
    analysis_id: &str,
    tile_id: &str,
    providers: &State<Providers>,
) -> Result<PngImage, Status> {
    let analysis_id = Uuid::parse_str(analysis_id).map_err(|_| Status::BadRequest)?;
    let Ok(tile_id) = TileId::parse(tile_id) else {
        return Err(Status::BadRequest);
    };

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
        return Err(Status::NoContent);
    };

    Ok(PngImage(png_bytes))
}

#[post("/fresnelSliceObj/<analysis_id>/<tile_id>")]
pub async fn get_fresnel_slice_obj(
    analysis_id: &str,
    tile_id: &str,
    providers: &State<Providers>,
) -> Result<(ContentType, TextStream![String]), Status> {
    let analysis_id = Uuid::parse_str(analysis_id).map_err(|_| Status::BadRequest)?;
    let Ok(tile_id) = TileId::parse(tile_id) else {
        return Err(Status::BadRequest);
    };

    let analysis = providers.point_eval_result_provider().get_full(
        &analysis_id,
        providers.elevation_tile_provider().as_ref(),
        providers.obstruction_provider().as_ref()
    ).await?;

    let obj_stream = TextStream! {
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
) -> Result<KmlDownload, Status> {
    let analysis_id = Uuid::parse_str(analysis_id).map_err(|_| Status::BadRequest)?;

    let analysis_outcome = providers.point_eval_result_provider().get_full(
        &analysis_id,
        providers.elevation_tile_provider().as_ref(),
        providers.obstruction_provider().as_ref()
    ).await?;

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
