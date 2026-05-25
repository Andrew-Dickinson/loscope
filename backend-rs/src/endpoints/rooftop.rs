use crate::building::bin_id::BINId;
use crate::building::heightmap::{RooftopHeightMapFactory};
use crate::providers::Providers;
use crate::sample_points::point::SamplePoints;
use crate::sample_points::sample_grid::sample_points_for_rooftop;
use futures_util::{StreamExt};
use rocket::http::{ContentType, Status};
use rocket::response::stream::TextStream;
use rocket::serde::json::Json;
use rocket::{State};
use serde::Deserialize;

#[get("/render/<bin_id>")]
pub async fn render_rooftop(
    bin_id: &str,
    providers: &State<Providers>,
) -> Result<(ContentType, TextStream![String]), Status> {
    let Ok(bin_id) = BINId::parse(bin_id) else {
        return Err(Status::BadRequest);
    };

    let factory = RooftopHeightMapFactory::new(
        providers.footprint_provider().as_ref(),
        providers.elevation_tile_provider().as_ref(),
    );
    let heightmap = factory.create(bin_id).await.map_err(|e| {
        eprintln!("{:?}", e);
        e
    })?;

    let obj_stream = TextStream! {
        let mut stream = std::pin::pin!(heightmap.to_rooftop_obj_stream());
        while let Some(chunk) = stream.next().await {
            yield chunk;
        }
    };

    Ok((ContentType::new("model", "obj"), obj_stream))
}

#[derive(Deserialize)]
pub struct SampleConfig {
    mast_offset_ft: f64,
    sample_spacing: Option<f64>,
}

#[post("/samplePoints/<bin_id>", format = "json", data = "<sample_config>")]
pub async fn sample_points(
    bin_id: &str,
    sample_config: Json<SampleConfig>,
    providers: &State<Providers>,
) -> Result<Json<SamplePoints>, Status> {
    let Ok(bin_id) = BINId::parse(bin_id) else {
        return Err(Status::BadRequest);
    };

    if sample_config
        .sample_spacing
        .is_some_and(|spacing| spacing < 1.0)
    {
        return Err(Status::BadRequest);
    }

    let factory = RooftopHeightMapFactory::new(
        providers.footprint_provider().as_ref(),
        providers.elevation_tile_provider().as_ref(),
    );
    let heightmap = factory.create(bin_id).await.map_err(|e| {
        eprintln!("{:?}", e);
        e
    })?;

    Ok(Json(SamplePoints::new(
        sample_config
            .sample_spacing
            .map(|spacing| {
                sample_points_for_rooftop(&heightmap, sample_config.mast_offset_ft, spacing)
            })
            .unwrap_or_default(),
        heightmap.sw_offset().clone(),
    )))
}
