use std::pin::Pin;
use std::str::FromStr;
use futures_util::{Stream, StreamExt};
use rocket::http::{ContentType, MediaType, Status};
use rocket::response::stream::TextStream;
use rocket::{Response, State};
use wkt::ToWkt;
use crate::building::heightmap::{get_intersecting_tiles, BINId, RooftopHeightMapFactory};
use crate::providers::Providers;
use crate::types::coords::{NYSCoords3};
use crate::types::errors::AssetErr;
use rocket::response::Debug;

#[get("/render/<bin_id>")]
pub async fn render_rooftop<'a>(
    bin_id: &str,
    providers: &State<Providers>
) -> Result<(ContentType, TextStream![String]), Status>  {
    // TODO: Online lookup to validate it's a real BIN?
    let Ok(bin_id) = BINId::parse(bin_id) else { return Err(Status::BadRequest) };

    let factory = RooftopHeightMapFactory::new(
        providers.footprint_provider().as_ref(),
        providers.elevation_tile_provider().as_ref()
    );
    let heightmap = factory.create(bin_id).await
        .map_err(|e| {eprintln!("{:?}", e); e})?;

    let obj_stream = TextStream! {
        let mut stream = std::pin::pin!(heightmap.to_rooftop_obj_stream());
        while let Some(chunk) = stream.next().await {
            yield chunk;
        }
    };

    Ok((ContentType::new("model", "obj"), obj_stream))
}