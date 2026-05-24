use rocket::http::Status;
use rocket::serde::json::Json;
use rocket::State;
use crate::providers::Providers;
use crate::types::errors::MeshDBError::{ApiError, DataError};
use crate::types::meshdb::NumberLookupResponse;

#[get("/resolve-number/<number>")]
pub async fn resolve_number(number: &str, providers: &State<Providers>) -> Result<Json<NumberLookupResponse>, Status> {
    let Ok(parsed_number) = number.parse::<u32>() else {
        return Err(Status::BadRequest);
    };

    providers.meshdb_provider().resolve_nn_or_install_to_bin(parsed_number).await
        .map_err(|e| match e {
            ApiError(_) => Status::BadGateway,
            DataError(_) => Status::NotFound
        })
        .map(Json)
}