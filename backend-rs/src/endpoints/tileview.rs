use crate::providers::Providers;
use crate::types::obstructions::{ObstructionId, ObstructionMeta, ObstructionType};
use crate::types::tiles::{SUBGRID_TILE_SIDE_LENGTH_USFT, TileId};
use futures_util::StreamExt;
use crate::util::image_adjustments::{apply_photo_adjustments, colorize_from_classifications};
use image::ImageFormat;
use rocket::State;
use rocket::http::{ContentType, Status};
use rocket::response::stream::TextStream;
use rocket::serde::json::Json;
use rocket::serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Cursor;

#[derive(Responder)]
#[response(status = 200, content_type = "image/jpeg")]
pub struct JpegImage(Vec<u8>);

#[derive(Responder)]
#[response(status = 200, content_type = "image/tiff")]
pub struct TiffImage(Vec<u8>);

#[derive(Deserialize, Serialize)]
pub struct TerrainTileOverview {
    obstruction_ids: HashMap<ObstructionType, Vec<ObstructionId>>,
}

#[get("/terrain/tileOverview/<tile_id>")]
pub async fn get_terrain_tile_overview(
    tile_id: &str,
    providers: &State<Providers>,
) -> Result<Json<TerrainTileOverview>, Status> {
    let Ok(tile_id) = TileId::parse(tile_id) else {
        return Err(Status::BadRequest);
    };

    // TODO: Fetch street geometry and use it to label street names in the 3d view?

    // TODO: Add obstructions for bridge tiles/outlines

    // TODO: Obstructions for active waterways and rail lines?

    Ok(Json(TerrainTileOverview {
        obstruction_ids: providers
            .obstruction_provider()
            .get_obstruction_ids_for_tile(tile_id)
            .await?,
    }))
}

#[get("/terrain/heightRaster/<tile_id>")]
pub async fn get_terrain_raster(
    tile_id: &str,
    providers: &State<Providers>,
) -> Result<TiffImage, Status> {
    let Ok(tile_id) = TileId::parse(tile_id) else {
        return Err(Status::BadRequest);
    };
    // TODO: Would it be better to use a CDN style direct browser file access for this?
    let tile = providers
        .elevation_tile_provider()
        .get_elevation_tile(tile_id)
        .await?;

    let width = SUBGRID_TILE_SIDE_LENGTH_USFT as usize;
    let mut tiff_bytes = Vec::<u8>::with_capacity(2 * width * width);
    tile.write_to_tiff(Cursor::new(&mut tiff_bytes))
        .map_err(|_| Status::InternalServerError)?;
    Ok(TiffImage(tiff_bytes))
}

#[get("/terrain/obstructionOverview/<obstruction_type>/<obstruction_id>")]
pub async fn get_terrain_obstruction_meta(
    obstruction_type: &str,
    obstruction_id: &str,
    providers: &State<Providers>,
) -> Result<Json<ObstructionMeta>, Status> {
    let obstruction_id: ObstructionId =
        ObstructionId::parse_str(obstruction_id).map_err(|_| Status::BadRequest)?;
    let obstruction_type: ObstructionType =
        ObstructionType::parse(obstruction_type).map_err(|_| Status::BadRequest)?;

    Ok(providers
        .obstruction_provider()
        .get_obstruction_meta(&obstruction_type, obstruction_id)
        .await
        .map(Json)?)
}

#[get("/terrain/obstructionObj/<obstruction_type>/<obstruction_id>/<tile_id>")]
pub async fn get_terrain_obstruction_obj(
    obstruction_type: &str,
    obstruction_id: &str,
    tile_id: &str,
    providers: &State<Providers>,
) -> Result<(ContentType, TextStream![String]), Status> {
    let obstruction_id: ObstructionId =
        ObstructionId::parse_str(obstruction_id).map_err(|_| Status::BadRequest)?;
    let obstruction_type: ObstructionType =
        ObstructionType::parse(obstruction_type).map_err(|_| Status::BadRequest)?;
    let Ok(tile_id) = TileId::parse(tile_id) else {
        return Err(Status::BadRequest);
    };

    // TODO: Would it be better to use a CDN style direct browser file access for this?
    //  We would need to pre-create the OBJ files, and somehow embed the xy offset for the browser to apply relative
    //  to the terrain mesh

    let (meta, obstruction) = tokio::try_join!(
        providers
            .obstruction_provider()
            .get_obstruction_meta(&obstruction_type, obstruction_id),
        providers
            .obstruction_provider()
            .get_obstruction_raster(&obstruction_type, obstruction_id),
    )?;

    let tile_sw = tile_id.get_sw_corner();
    let x_offset = (*meta.sw_offset().easting() - *tile_sw.easting()) as isize;
    let y_offset = (*meta.sw_offset().northing() - *tile_sw.northing()) as isize;

    let obj_stream = TextStream! {
        let mut stream = std::pin::pin!(
            obstruction.to_obj_stream(obstruction_type.clone(), obstruction_id, x_offset, y_offset)
        );
        while let Some(chunk) = stream.next().await {
            yield chunk;
        }
    };

    Ok((ContentType::new("model", "obj"), obj_stream))
}

#[get("/terrain/orthoImage/<tile_id>")]
pub async fn get_terrain_ortho(
    tile_id: &str,
    providers: &State<Providers>,
) -> Result<JpegImage, Status> {
    // TODO: Would it be better to use a CDN style direct browser file access for this?
    //   Especially since the requests will be balanced across many workers, which may or may
    //   not have shared cache storage
    let Ok(tile_id) = TileId::parse(tile_id) else {
        return Err(Status::BadRequest);
    };

    let ortho_img = providers.ortho_provider().get_ortho(tile_id).await?;
    let ortho_img = apply_photo_adjustments(ortho_img);

    let classification_tile = providers.terrain_classification_provider()
        .get_terrain_classification_tile(tile_id).await
        .map_err(|err| {println!("{err}"); err})?;
    let ortho_img = colorize_from_classifications(ortho_img, classification_tile);

    let mut jpeg_bytes: Vec<u8> = Vec::new();
    ortho_img
        .write_to(&mut Cursor::new(&mut jpeg_bytes), ImageFormat::Jpeg)
        .unwrap();

    Ok(JpegImage(jpeg_bytes))
}
