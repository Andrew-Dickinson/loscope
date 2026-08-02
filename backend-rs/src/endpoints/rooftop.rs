use std::collections::HashSet;
use crate::analysis::memory_budget::MemoryBudget;
use crate::analysis::memory_estimate::{elevation_tile_endpoint_bytes, estimate_heightmap_bytes};
use crate::analysis::memory_paranoid;
use crate::building::background_tiles::zero_footprint_pixels;
use crate::building::bin_id::BINId;
use crate::building::heightmap::{RooftopHeightMapFactory, get_intersecting_tiles, heightmap_pixel_dims};
use crate::endpoints::api_error::ApiError;
use crate::providers::Providers;
use crate::sample_points::point::SamplePoints;
use crate::sample_points::sample_grid::sample_points_for_rooftop;
use crate::types::coords::NYSCoords2;
use crate::types::tiles::TileId;
use crate::util::coord_conversion::with_coord_converter;
use futures_util::{StreamExt};
use rocket::http::{ContentType, Status};
use rocket::response::stream::TextStream;
use rocket::serde::json::Json;
use rocket::{State};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use geo::{Buffer, MapCoords};
use geojson::{Feature, Geometry, GeometryValue, JsonObject, JsonValue};

const BACKGROUND_TILE_BUFFER_ZONE_USFT: f64 = 250.0;

#[get("/render/<bin_id>")]
pub async fn render_rooftop(
    bin_id: &str,
    providers: &State<Providers>,
    memory_budget: &State<MemoryBudget>,
) -> Result<(ContentType, TextStream![String]), ApiError> {
    let Ok(bin_id) = BINId::parse(bin_id) else {
        return Err(Status::BadRequest.into());
    };

    // Fetch the (WKT of the) footprint up front (cheap — cached) so we can size-check before
    // the heavy heightmap allocation. The factory below re-fetches it, which hits the same cache.
    let footprint = providers.footprint_provider().get_footprint(bin_id).await?;
    let (_, poly_bounds) = get_intersecting_tiles(&footprint)?;
    let (w, h) = heightmap_pixel_dims(&poly_bounds);

    // Held until the response stream (which owns the heightmap data) finishes, not just until
    // this handler returns — released early would let the budget think this memory is free
    // while it's still resident and being streamed to the client.
    let heightmap_estimate = estimate_heightmap_bytes(w, h);
    let reservation = memory_budget.try_reserve(heightmap_estimate)?;

    let factory = RooftopHeightMapFactory::new(
        providers.footprint_provider().as_ref(),
        providers.elevation_tile_provider().as_ref(),
    );
    let heightmap = memory_paranoid::scope("render_rooftop", heightmap_estimate, factory.create(bin_id))
        .await
        .map_err(|e| {
            eprintln!("{:?}", e);
            e
        })?;

    let obj_stream = TextStream! {
        let _reservation = reservation;
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
    memory_budget: &State<MemoryBudget>,
) -> Result<Json<SamplePoints>, ApiError> {
    let Ok(bin_id) = BINId::parse(bin_id) else {
        return Err(Status::BadRequest.into());
    };

    if sample_config
        .sample_spacing
        .is_some_and(|spacing| spacing < 1.0)
    {
        return Err(Status::BadRequest.into());
    }

    let footprint = providers.footprint_provider().get_footprint(bin_id).await?;
    let (_, poly_bounds) = get_intersecting_tiles(&footprint)?;
    let (w, h) = heightmap_pixel_dims(&poly_bounds);
    // Released when this function returns, which is after the heightmap has already been
    // consumed into the (much smaller) sample point list below — no streaming to worry about.
    let heightmap_estimate = estimate_heightmap_bytes(w, h);
    let _reservation = memory_budget.try_reserve(heightmap_estimate)?;

    let factory = RooftopHeightMapFactory::new(
        providers.footprint_provider().as_ref(),
        providers.elevation_tile_provider().as_ref(),
    );
    let heightmap = memory_paranoid::scope("sample_points", heightmap_estimate, factory.create(bin_id))
        .await
        .map_err(|e| {
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

#[derive(Serialize)]
pub struct BackgroundTileInfo {
    id: TileId,
    sw_nys: NYSCoords2,
}

#[derive(Serialize)]
pub struct BackgroundTileIds {
    tiles: Vec<BackgroundTileInfo>,
}

#[get("/backgroundTileIds/<bin_id>")]
pub async fn background_tile_ids(
    bin_id: &str,
    providers: &State<Providers>,
) -> Result<Json<BackgroundTileIds>, Status> {
    let Ok(bin_id) = BINId::parse(bin_id) else {
        return Err(Status::BadRequest);
    };

    let tile_ids: HashSet<TileId> = providers
        .footprint_provider()
        .get_footprint(bin_id)
        .await
        .map_err(|e| {
            eprintln!("{:?}", e);
            Status::from(e)
        })?
        .buffer(BACKGROUND_TILE_BUFFER_ZONE_USFT)
        .iter()
        .map(|poly| get_intersecting_tiles(poly).map(|(tiles, _)| tiles))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
            eprintln!("{:?}", e);
            Status::from(e)
        })?
        .into_iter()
        .flatten()
        .collect();

    let tiles = tile_ids
        .into_iter()
        .map(|tile_id| BackgroundTileInfo {
            sw_nys: tile_id.get_sw_corner(),
            id: tile_id,
        })
        .collect();

    Ok(Json(BackgroundTileIds { tiles }))
}

#[get("/footprintGeoJson/<bin_id>")]
pub async fn footprint_geojson(
    bin_id: &str,
    providers: &State<Providers>,
) -> Result<(ContentType, Json<Feature>), Status> {
    let Ok(bin_id) = BINId::parse(bin_id) else {
        return Err(Status::BadRequest);
    };

    let footprint_nys = providers.footprint_provider().get_footprint(bin_id).await?;

    // GeoJSON coordinates are (lon, lat) — matches (x, y) here since we map straight from the
    // NYS State Plane (x=easting, y=northing) polygon to WGS84 (x=lon, y=lat).
    let footprint_wgs84 = with_coord_converter(|converter| {
        footprint_nys.map_coords(|c| {
            let gps = converter.to_gps2(&NYSCoords2::new(c.x, c.y));
            geo::coord! { x: *gps.lon(), y: *gps.lat() }
        })
    });

    let mut feature = Feature::from(Geometry::new(GeometryValue::from(&footprint_wgs84)));
    let mut properties = JsonObject::new();
    properties.insert(
        "bin".to_string(),
        JsonValue::String(bin_id.as_str().to_string()),
    );
    feature.properties = Some(properties);

    Ok((ContentType::new("application", "geo+json"), Json(feature)))
}

#[derive(Responder)]
#[response(status = 200, content_type = "image/tiff")]
pub struct TiffImage(Vec<u8>);

#[get("/backgroundTileRaster/<bin_id>/<tile_id>")]
pub async fn background_tile_raster(
    bin_id: &str,
    tile_id: &str,
    providers: &State<Providers>,
    memory_budget: &State<MemoryBudget>,
) -> Result<TiffImage, ApiError> {
    let Ok(bin_id) = BINId::parse(bin_id) else {
        return Err(Status::BadRequest.into());
    };
    let Ok(tile_id) = TileId::parse(tile_id) else {
        return Err(Status::BadRequest.into());
    };

    // One footprint (small) + one fixed-size elevation tile — bounded, so a flat reservation
    // covers this regardless of which building/tile is requested.
    let tile_estimate = elevation_tile_endpoint_bytes();
    let _reservation = memory_budget.try_reserve(tile_estimate)?;

    let tiff_bytes = memory_paranoid::scope("background_tile_raster", tile_estimate, async {
        let (footprint, mut tile) = tokio::try_join!(
            async {
                providers
                    .footprint_provider()
                    .get_footprint(bin_id)
                    .await
                    .map_err(|e| {
                        eprintln!("{:?}", e);
                        ApiError::from(e)
                    })
            },
            async {
                providers
                    .elevation_tile_provider()
                    .get_elevation_tile(tile_id)
                    .await
                    .map_err(|e| {
                        eprintln!("{:?}", e);
                        ApiError::from(e)
                    })
            },
        )?;

        tile.mutate_elevation_values(move |elevation_inches| {
            zero_footprint_pixels(&footprint, tile_id, elevation_inches);
        });

        let mut tiff_bytes = Vec::<u8>::new();
        tile.write_to_tiff(Cursor::new(&mut tiff_bytes))
            .map_err(|_| ApiError::new(Status::InternalServerError))?;
        memory_paranoid::check("background_tile_raster::tiff_bytes", tiff_bytes.len() as u64);

        Ok::<Vec<u8>, ApiError>(tiff_bytes)
    })
    .await?;

    Ok(TiffImage(tiff_bytes))
}
