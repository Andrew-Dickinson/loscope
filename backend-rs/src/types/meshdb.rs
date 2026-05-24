use derive_new::new;
use serde::{Deserialize, Serialize};
use crate::building::bin_id::BINId;

#[derive(new,Serialize,Deserialize)]
pub enum MeshdbBINSource {
    NN,
    Install
}

#[derive(new,Serialize,Deserialize)]
pub struct NumberLookupResponse {
    bin: BINId,
    source: MeshdbBINSource
}