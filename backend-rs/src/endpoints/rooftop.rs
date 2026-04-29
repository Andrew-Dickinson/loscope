use rocket::http::Status;
use rocket::State;
use wkt::ToWkt;
use crate::building::heightmap::BINId;
use crate::providers::Providers;
use crate::types::coords::{NYSCoords3};
use crate::types::errors::AssetErr;

#[derive(Responder)]
#[response(status = 200, content_type = "model/obj")]
pub struct ObjModel(Vec<u8>);


#[get("/render/<bin_id>")]
pub async fn render_rooftop(
    bin_id: &str,
    providers: &State<Providers>
) -> Result<String, Status> {
// ) -> Result<ObjModel, Status> {
    // TODO: Online lookup to validate it's a real BIN?
    let Ok(bin_id) = BINId::parse(bin_id) else { return Err(Status::BadRequest) };

    let footprint = providers.footprint_provider().get_footprint(&bin_id).await?;


    // TODO: Impelmetn me
    // https://docs.rs/wkt/latest/wkt/ ?

    Ok(footprint.wkt_string())
}