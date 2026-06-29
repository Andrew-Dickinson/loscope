use crate::analysis::point_evaluation::{PointEvaluationOutcome, PointEvaluationOutcomeFull, PointEvaluationOutput};
use crate::providers::backends::value_store::ValueStore;
use crate::types::errors::AssetErr;
use derive_new::new;
use rocket::http::Status;
use uuid::Uuid;
use crate::analysis::map_overview::PointEvaluationOverview;
use crate::providers::elevation_tile_provider::ElevationTileProvider;
use crate::providers::obstruction_provider::ObstructionProvider;

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
            wincode::config::serialize(
                result,
                wincode::config::Configuration::default().disable_preallocation_size_limit(),
            )
            .map_err(|e| {
                AssetErr::AssetContentError(format!(
                    "Error serializing for result_id {}: {e}",
                    result.output().id()
                ))
            })?,
        )
    }

    pub fn get(&self, result_id: &Uuid) -> Result<PointEvaluationOutcome, AssetErr> {
        let resp = self
            .value_store
            .get(PointEvaluationResultProvider::key(result_id))?;
        wincode::config::deserialize::<PointEvaluationOutcome, _>(
            &resp,
            wincode::config::Configuration::default().disable_preallocation_size_limit(),
        )
        .map_err(|e| {
            AssetErr::AssetContentError(format!(
                "Error deserializing response for result_id {result_id}: {e}"
            ))
        })
    }

    pub async fn get_full(
        &self,
        result_id: &Uuid,
        tile_provider: &(dyn ElevationTileProvider + Send + Sync),
        obstruction_provider: &(dyn ObstructionProvider + Send + Sync)
    ) -> Result<PointEvaluationOutcomeFull, AssetErr> {
        let analysis_details = self.get(result_id)?;

        match analysis_details {
            PointEvaluationOutcome::Lite(lite) => {
                let full = lite.to_full(tile_provider, obstruction_provider).await?;
                let enveloped = PointEvaluationOutcome::Full(Box::new(full));
                self.put(&enveloped)?;
                if let PointEvaluationOutcome::Full(full) = enveloped {
                    Ok(*full)
                } else {
                    panic!(
                        "PointEvaluationOutcome::Full() will always un-envelope to PointEvaluationOutcome::Full"
                    )
                }
            },
            PointEvaluationOutcome::Full(full) => Ok(*full),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::fresnel_zone::FresnelZonePoint;
    use crate::analysis::point_evaluation::{
        IntersectionResult, PointEvaluationInput, PointEvaluationOutcome, PointEvaluationOutcomeFull,
        PointEvaluationOutcomeLite, PointEvaluationOutput, ResultStatus, ZoneEvaluation,
    };
    use crate::providers::backends::value_store::InMemoryValueStore;
    use crate::providers::elevation_tile_provider::{ElevationTile, ElevationTileProvider};
    use crate::providers::obstruction_provider::ObstructionProvider;
    use crate::types::coords::{NYSCoords2, NYSCoords3};
    use crate::types::obstructions::{ObstructionId, ObstructionMeta, ObstructionRaster, ObstructionType};
    use crate::types::obstructions::ObstructionTypesFilter;
    use crate::types::stairstep::StairStepGrid;
    use crate::types::tiles::TileId;
    use async_trait::async_trait;
    use ndarray::{Array1, Array2};
    use std::collections::{HashMap, HashSet};
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    // ── Shared test helpers ───────────────────────────────────────────────────

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

    fn make_output(id: Uuid) -> PointEvaluationOutput {
        let input = PointEvaluationInput::new(
            NYSCoords3::new(1_039_748.0, 176_148.0, 0.0),
            NYSCoords3::new(1_040_000.0, 176_500.0, 0.0),
            2_400_000_000.0,
            ObstructionTypesFilter::All,
        );
        PointEvaluationOutput::new(id, input, ResultStatus::Unobstructed)
    }

    fn make_lite_outcome(id: Uuid) -> PointEvaluationOutcome {
        PointEvaluationOutcome::Lite(PointEvaluationOutcomeLite::new(make_output(id), HashSet::new()))
    }

    fn make_full_outcome(id: Uuid) -> PointEvaluationOutcome {
        PointEvaluationOutcome::Full(Box::new(PointEvaluationOutcomeFull::new(
            make_output(id),
            empty_zone(),
            empty_zone(),
            HashSet::new(),
        )))
    }

    fn in_memory_provider() -> PointEvaluationResultProvider {
        PointEvaluationResultProvider::new(Box::new(InMemoryValueStore::new()))
    }

    // ── Mock providers for get_full tests ────────────────────────────────────

    struct FlatTileProvider;

    #[async_trait]
    impl ElevationTileProvider for FlatTileProvider {
        async fn get_elevation_tile(&self, tile_id: TileId) -> Result<ElevationTile, AssetErr> {
            let side = usize::from(crate::types::tiles::SUBGRID_TILE_SIDE_LENGTH_USFT);
            Ok(ElevationTile::new(tile_id, Array2::from_elem((side, side), 0u16)))
        }
    }

    struct EmptyObstructionProvider;

    #[async_trait]
    impl ObstructionProvider for EmptyObstructionProvider {
        async fn get_obstruction_ids_for_tile(&self, _: TileId) -> Result<HashMap<ObstructionType, Vec<ObstructionId>>, AssetErr> {
            Ok(HashMap::new())
        }
        async fn get_obstruction_meta(&self, _: &ObstructionType, _: ObstructionId) -> Result<ObstructionMeta, AssetErr> {
            unreachable!()
        }
        async fn get_obstruction_raster(&self, _: &ObstructionType, _: ObstructionId) -> Result<ObstructionRaster, AssetErr> {
            unreachable!()
        }
    }

    // ── Roundtrip: Lite variant ───────────────────────────────────────────────

    #[test]
    fn put_then_get_lite_roundtrip_preserves_id_and_frequency() {
        let id = Uuid::new_v4();
        let p = in_memory_provider();
        p.put(&make_lite_outcome(id)).unwrap();
        let got = p.get(&id).unwrap();
        assert_eq!(*got.output().id(), id);
        assert_eq!(*got.output().input().frequency_hz(), 2_400_000_000.0);
    }

    #[test]
    fn lite_outcome_roundtrips_as_lite_variant() {
        let id = Uuid::new_v4();
        let p = in_memory_provider();
        p.put(&make_lite_outcome(id)).unwrap();
        assert!(matches!(p.get(&id).unwrap(), PointEvaluationOutcome::Lite(_)));
    }

    // ── Roundtrip: Full variant ───────────────────────────────────────────────

    #[test]
    fn put_then_get_full_roundtrip_preserves_id_and_frequency() {
        let id = Uuid::new_v4();
        let p = in_memory_provider();
        p.put(&make_full_outcome(id)).unwrap();
        let got = p.get(&id).unwrap();
        assert_eq!(*got.output().id(), id);
        assert_eq!(*got.output().input().frequency_hz(), 2_400_000_000.0);
    }

    #[test]
    fn full_outcome_roundtrips_as_full_variant() {
        let id = Uuid::new_v4();
        let p = in_memory_provider();
        p.put(&make_full_outcome(id)).unwrap();
        assert!(matches!(p.get(&id).unwrap(), PointEvaluationOutcome::Full(_)));
    }

    // ── Multiple outcomes are independent ────────────────────────────────────

    #[test]
    fn multiple_outcomes_stored_and_retrieved_independently() {
        let id_a = Uuid::new_v4();
        let id_b = Uuid::new_v4();
        let p = in_memory_provider();
        p.put(&make_lite_outcome(id_a)).unwrap();
        p.put(&make_lite_outcome(id_b)).unwrap();
        assert_eq!(*p.get(&id_a).unwrap().output().id(), id_a);
        assert_eq!(*p.get(&id_b).unwrap().output().id(), id_b);
    }

    // ── Missing / wrong key ───────────────────────────────────────────────────

    #[test]
    fn get_unknown_id_returns_asset_not_found() {
        let err = in_memory_provider().get(&Uuid::new_v4()).err().unwrap();
        assert!(matches!(err, AssetErr::AssetNotFound(_)));
    }

    #[test]
    fn get_wrong_id_after_put_returns_asset_not_found() {
        let p = in_memory_provider();
        p.put(&make_lite_outcome(Uuid::new_v4())).unwrap();
        let err = p.get(&Uuid::new_v4()).err().unwrap();
        assert!(matches!(err, AssetErr::AssetNotFound(_)));
    }

    // ── Key format ───────────────────────────────────────────────────────────

    struct KeyCapturingStore {
        inner: InMemoryValueStore,
        last_put_key: Arc<Mutex<Option<String>>>,
    }

    impl KeyCapturingStore {
        fn new() -> (Self, Arc<Mutex<Option<String>>>) {
            let captured = Arc::new(Mutex::new(None));
            let store = Self {
                inner: InMemoryValueStore::new(),
                last_put_key: Arc::clone(&captured),
            };
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
        p.put(&make_lite_outcome(id)).unwrap();
        assert_eq!(
            captured.lock().unwrap().as_deref(),
            Some(format!("PointEvaluationResult/{id}").as_str()),
        );
    }

    // ── Corrupted data ────────────────────────────────────────────────────────

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

    // ── get_full: already-Full path ───────────────────────────────────────────

    #[tokio::test]
    async fn get_full_on_stored_full_returns_correct_id() {
        let id = Uuid::new_v4();
        let p = in_memory_provider();
        p.put(&make_full_outcome(id)).unwrap();
        let full = p.get_full(&id, &FlatTileProvider, &EmptyObstructionProvider).await.unwrap();
        assert_eq!(*full.output().id(), id);
    }

    #[tokio::test]
    async fn get_full_on_stored_full_does_not_recompute() {
        // Providers panic if called — confirms no recomputation happens for an already-Full result.
        struct PanickingTileProvider;
        #[async_trait]
        impl ElevationTileProvider for PanickingTileProvider {
            async fn get_elevation_tile(&self, _: TileId) -> Result<ElevationTile, AssetErr> {
                panic!("tile provider should not be consulted for a cached Full result")
            }
        }
        let id = Uuid::new_v4();
        let p = in_memory_provider();
        p.put(&make_full_outcome(id)).unwrap();
        let full = p.get_full(&id, &PanickingTileProvider, &EmptyObstructionProvider).await.unwrap();
        assert_eq!(*full.output().id(), id);
    }

    // ── get_full: Lite → recompute → upgrade path ─────────────────────────────

    #[tokio::test]
    async fn get_full_on_stored_lite_recomputes_and_returns_correct_id() {
        let id = Uuid::new_v4();
        let p = in_memory_provider();
        p.put(&make_lite_outcome(id)).unwrap();
        let full = p.get_full(&id, &FlatTileProvider, &EmptyObstructionProvider).await.unwrap();
        assert_eq!(*full.output().id(), id);
        assert_eq!(*full.output().input().frequency_hz(), 2_400_000_000.0);
    }

    #[tokio::test]
    async fn get_full_on_stored_lite_upgrades_stored_value_to_full() {
        let id = Uuid::new_v4();
        let p = in_memory_provider();
        p.put(&make_lite_outcome(id)).unwrap();
        p.get_full(&id, &FlatTileProvider, &EmptyObstructionProvider).await.unwrap();
        assert!(matches!(p.get(&id).unwrap(), PointEvaluationOutcome::Full(_)));
    }

    // ── get_full: missing key ─────────────────────────────────────────────────

    #[tokio::test]
    async fn get_full_on_missing_key_returns_not_found() {
        let p = in_memory_provider();
        let err = p.get_full(&Uuid::new_v4(), &FlatTileProvider, &EmptyObstructionProvider).await.err().unwrap();
        assert!(matches!(err, AssetErr::AssetNotFound(_)));
    }
}
