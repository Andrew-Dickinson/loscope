use derive_new::new;
use uuid::Uuid;
use crate::analysis::point_evaluation::PointEvaluationOutcome;
use crate::providers::backends::value_store::ValueStore;
use crate::types::errors::AssetErr;

const KEY_PREFIX: &str = "PointEvaluationResult";

#[derive(new)]
pub struct PointEvaluationResultProvider {
    value_store: Box<dyn ValueStore + Send + Sync>,
}

impl PointEvaluationResultProvider {
    fn key(result_id: &Uuid) -> String {
        format!("{KEY_PREFIX}/{result_id}")
    }

    pub fn put(&self, result: &PointEvaluationOutcome) -> Result<(), AssetErr> {
        self.value_store.put(
            PointEvaluationResultProvider::key(result.output().id()),
            wincode::config::serialize(result, wincode::config::Configuration::default().disable_preallocation_size_limit())
                .map_err(|e| AssetErr::AssetContentError(
                    format!("Error serializing for result_id {}: {e}", result.output().id())
                ))?
        )
    }

    pub fn get(&self, result_id: &Uuid) -> Result<PointEvaluationOutcome, AssetErr> {
        let resp = self.value_store.get(PointEvaluationResultProvider::key(result_id))?;
        wincode::config::deserialize::<PointEvaluationOutcome, _>(&resp, wincode::config::Configuration::default().disable_preallocation_size_limit())
            .map_err(|e| AssetErr::AssetContentError(
                format!("Error deserializing response for result_id {result_id}: {e}")
            ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};
    use ndarray::{Array1, Array2};
    use uuid::Uuid;
    use super::*;
    use crate::analysis::fresnel_zone::FresnelZonePoint;
    use crate::analysis::point_evaluation::{
        IntersectionResult, PointEvaluationInput,
        PointEvaluationOutcome, PointEvaluationOutput, ResultStatus, ZoneEvaluation,
    };
    use crate::providers::backends::value_store::InMemoryValueStore;
    use crate::types::coords::{NYSCoords2, NYSCoords3};
    use crate::types::obstructions::ObstructionTypesFilter;
    use crate::types::stairstep::StairStepGrid;

    fn empty_zone() -> ZoneEvaluation {
        let base = NYSCoords2::new(0.0, 0.0);
        let zone = StairStepGrid::<FresnelZonePoint>::new(
            Array2::default((1, 1)),
            Array1::from_vec(vec![0]),
            Array1::from_vec(vec![0]),
            base.clone(),
        );
        let intersection: IntersectionResult = StairStepGrid::new(
            Array2::default((1, 1)),
            Array1::from_vec(vec![0]),
            Array1::from_vec(vec![0]),
            base,
        );
        ZoneEvaluation::new(zone, intersection)
    }

    fn make_outcome(id: Uuid) -> PointEvaluationOutcome {
        let input = PointEvaluationInput::new(
            NYSCoords3::new(1_039_748.0, 176_148.0, 0.0),
            NYSCoords3::new(1_040_000.0, 176_500.0, 0.0),
            2_400_000_000.0,
            ObstructionTypesFilter::All,
        );
        let output = PointEvaluationOutput::new(id, input, ResultStatus::Unobstructed);
        PointEvaluationOutcome::new(output, empty_zone(), empty_zone(), HashSet::new())
    }

    fn in_memory_provider() -> PointEvaluationResultProvider {
        PointEvaluationResultProvider::new(Box::new(InMemoryValueStore::new()))
    }

    // --- roundtrip ---

    #[test]
    fn put_then_get_roundtrip_preserves_id_and_frequency() {
        let id = Uuid::new_v4();
        let p = in_memory_provider();
        p.put(&make_outcome(id)).unwrap();
        let got = p.get(&id).unwrap();
        assert_eq!(*got.output().id(), id);
        assert_eq!(*got.output().input().frequency_hz(), 2_400_000_000.0);
    }

    #[test]
    fn multiple_outcomes_stored_and_retrieved_independently() {
        let id_a = Uuid::new_v4();
        let id_b = Uuid::new_v4();
        let p = in_memory_provider();
        p.put(&make_outcome(id_a)).unwrap();
        p.put(&make_outcome(id_b)).unwrap();
        assert_eq!(*p.get(&id_a).unwrap().output().id(), id_a);
        assert_eq!(*p.get(&id_b).unwrap().output().id(), id_b);
    }

    // --- missing / wrong key ---

    #[test]
    fn get_unknown_id_returns_asset_not_found() {
        let err = in_memory_provider().get(&Uuid::new_v4()).err().unwrap();
        assert!(matches!(err, AssetErr::AssetNotFound(_)));
    }

    #[test]
    fn get_wrong_id_after_put_returns_asset_not_found() {
        let p = in_memory_provider();
        p.put(&make_outcome(Uuid::new_v4())).unwrap();
        let err = p.get(&Uuid::new_v4()).err().unwrap();
        assert!(matches!(err, AssetErr::AssetNotFound(_)));
    }

    // --- key format ---

    struct KeyCapturingStore {
        inner: InMemoryValueStore,
        last_put_key: Arc<Mutex<Option<String>>>,
    }

    impl KeyCapturingStore {
        fn new() -> (Self, Arc<Mutex<Option<String>>>) {
            let captured = Arc::new(Mutex::new(None));
            let store = Self { inner: InMemoryValueStore::new(), last_put_key: Arc::clone(&captured) };
            (store, captured)
        }
    }

    impl ValueStore for KeyCapturingStore {
        fn put(&self, key: String, value: Vec<u8>) -> Result<(), AssetErr> {
            *self.last_put_key.lock().unwrap() = Some(key.clone());
            self.inner.put(key, value)
        }
        fn get(&self, key: String) -> Result<Vec<u8>, AssetErr> {
            self.inner.get(key)
        }
    }

    #[test]
    fn put_uses_prefixed_uuid_key() {
        let id = Uuid::new_v4();
        let (store, captured) = KeyCapturingStore::new();
        let p = PointEvaluationResultProvider::new(Box::new(store));
        p.put(&make_outcome(id)).unwrap();
        assert_eq!(
            captured.lock().unwrap().as_deref(),
            Some(format!("PointEvaluationResult/{id}").as_str()),
        );
    }

    // --- corrupted data ---

    struct CorruptedValueStore;

    impl ValueStore for CorruptedValueStore {
        fn put(&self, _: String, _: Vec<u8>) -> Result<(), AssetErr> { Ok(()) }
        fn get(&self, _: String) -> Result<Vec<u8>, AssetErr> {
            Ok(vec![0xFF, 0xFF, 0xFF])
        }
    }

    #[test]
    fn get_corrupted_data_returns_asset_content_error() {
        let p = PointEvaluationResultProvider::new(Box::new(CorruptedValueStore));
        let err = p.get(&Uuid::new_v4()).err().unwrap();
        assert!(matches!(err, AssetErr::AssetContentError(_)));
    }
}