use rocket::http::Status;
use rocket::State;
use crate::building::heightmap::BINId;
use crate::providers::S3BackedProviders;
use crate::types::coords::{NYSCoords3};

#[derive(Responder)]
#[response(status = 200, content_type = "model/obj")]
pub struct ObjModel(Vec<u8>);


#[get("/render/<bin_id>")]
pub async fn render_rooftop(
    bin_id: &str,
    providers: &State<S3BackedProviders>
) -> Result<ObjModel, Status> {
    // TODO: Online lookup to validate it's a real BIN?
    let Ok(bin_id) = BINId::parse(bin_id) else { return Err(Status::BadRequest) };


    // TODO: Impelmetn me
    // https://docs.rs/wkt/latest/wkt/ ?

    Err(Status::BadRequest)
}