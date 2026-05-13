use derive_new::new;
use uuid::Uuid;
use crate::analysis::point_evaluation::PointEvaluationResult;
use crate::providers::backends::value_store::ValueStore;
use crate::types::errors::AssetErr;

use serde_cbor::{from_slice, to_vec};

const KEY_PREFIX: &str = "PointEvaluationResult";

#[derive(new)]
pub struct PointEvaluationResultProvider {
    value_store: Box<dyn ValueStore + Send + Sync>,
}

impl PointEvaluationResultProvider {
    fn key(result_id: &Uuid) -> String {
        format!("{KEY_PREFIX}/{result_id}")
    }

    pub fn put(&self, result: &PointEvaluationResult) -> Result<(), AssetErr> {
        self.value_store.put(
            PointEvaluationResultProvider::key(result.output().id()),
            to_vec(result)
                .or_else(|e| Err(
                    AssetErr::AssetContentError(
                        format!("Error serializing for result_id {}: {e}", result.output().id())
                    )
                ))?
        )
    }

    pub fn get(&self, result_id: &Uuid) -> Result<PointEvaluationResult, AssetErr> {
        let resp = self.value_store.get(PointEvaluationResultProvider::key(result_id))?;
        Ok(
            from_slice(resp.as_slice())
            .or_else(|e| Err(AssetErr::AssetContentError(
                format!("Error deserializing response for result_id {result_id}: {e}")
            )))?
        )
    }
}