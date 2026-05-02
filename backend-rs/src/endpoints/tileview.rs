use std::io::Cursor;
use rocket::State;
use rocket::http::Status;
use image::ImageFormat;
use crate::providers::ortho_provider::OrthoProvider;
use crate::providers::Providers;
use crate::types::errors::AssetErr;
use crate::types::tiles::TileId;

#[derive(Responder)]
#[response(status = 200, content_type = "image/jpeg")]
pub struct JpegImage(Vec<u8>);

#[get("/terrain/orthoImage/<tile_id>")]
pub async fn get_terrain_ortho(
    tile_id: &str,
    providers: &State<Providers>
) -> Result<JpegImage, Status> {
    // TODO: Would it be better to use a CDN style direct browser file access for this?
    //   Especially since the requests will be balanced across many workers, which may or may
    //   not have shared cache storage
    let Ok(tile_id) = TileId::parse(&tile_id) else { return Err(Status::BadRequest) };

    let ortho_img = providers.ortho_provider().get_ortho(tile_id).await?;

    let mut jpeg_bytes: Vec<u8> = Vec::new();
    ortho_img.write_to(&mut Cursor::new(&mut jpeg_bytes), ImageFormat::Jpeg).unwrap();

    Ok(JpegImage(jpeg_bytes))
}