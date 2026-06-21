use crate::building::bin_id::BINId;
use derive_new::new;
use serde::{Deserialize, Serialize};

#[derive(Debug, new, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeshdbBINSource {
    NN,
    Install,
}

#[derive(Debug, new, Serialize, Deserialize)]
pub struct NumberLookupResponse {
    bin: BINId,
    kind: MeshdbBINSource,
}
