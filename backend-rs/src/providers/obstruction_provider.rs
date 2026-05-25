use crate::providers::backends::asset_fetcher::AssetType;
use crate::providers::backends::fs_cache::AssetProvider;
use crate::types::errors::AssetErr;
use crate::types::obstructions::{
    ObstructionId, ObstructionMeta, ObstructionRaster, ObstructionType,
};
use crate::types::tiles::TileId;
use std::collections::HashMap;
use std::io::{BufReader};
use std::sync::Arc;

#[async_trait]
pub trait ObstructionProvider {
    async fn get_obstruction_ids_for_tile(
        &self,
        tile_id: TileId,
    ) -> Result<HashMap<ObstructionType, Vec<ObstructionId>>, AssetErr>;
    async fn get_obstruction_meta(
        &self,
        obstruction_type: &ObstructionType,
        obstruction_id: ObstructionId,
    ) -> Result<ObstructionMeta, AssetErr>;
    async fn get_obstruction_raster(
        &self,
        obstruction_type: &ObstructionType,
        obstruction_id: ObstructionId,
    ) -> Result<ObstructionRaster, AssetErr>;
}

pub struct CachingObstructionProvider {
    asset_provider: Arc<dyn AssetProvider + Send + Sync>,
    obstruction_index: HashMap<ObstructionType, HashMap<TileId, Vec<ObstructionId>>>,
}

impl CachingObstructionProvider {
    pub async fn new(
        asset_provider: Arc<dyn AssetProvider + Send + Sync>,
    ) -> Result<Self, AssetErr> {
        let mut obstruction_index = HashMap::new();

        for index_file_name in asset_provider
            .list_assets_of_type(AssetType::ObstructionIndex)
            .await?
        {
            let index_file = asset_provider
                .get_asset(AssetType::ObstructionIndex, &index_file_name)
                .await?;
            let reader = BufReader::new(index_file);

            let Some(obstruction_type) = index_file_name
                .strip_suffix(".json")
                .and_then(|t| ObstructionType::parse(t).ok())
            else {
                continue;
            };

            let index: HashMap<TileId, Vec<ObstructionId>> = serde_json::from_reader(reader)
                .map_err(|e| {
                    AssetErr::AssetContentError(format!(
                        "Error deserializing JSON index for obstruction type {}: {}",
                        obstruction_type, e
                    ))
                })?;
            obstruction_index.insert(obstruction_type, index);
        }

        Ok(Self {
            asset_provider,
            obstruction_index,
        })
    }
}

#[async_trait]
impl ObstructionProvider for CachingObstructionProvider {
    async fn get_obstruction_ids_for_tile(
        &self,
        tile_id: TileId,
    ) -> Result<HashMap<ObstructionType, Vec<ObstructionId>>, AssetErr> {
        let mut output: HashMap<ObstructionType, Vec<ObstructionId>> = HashMap::new();
        self.obstruction_index.iter().for_each(|(type_, tile_map)| {
            if let Some(obstructions) = tile_map.get(&tile_id) {
                output.insert(type_.clone(), obstructions.clone());
            }
        });

        Ok(output)
    }

    async fn get_obstruction_meta(
        &self,
        obstruction_type: &ObstructionType,
        obstruction_id: ObstructionId,
    ) -> Result<ObstructionMeta, AssetErr> {
        let obstruction_meta_file = self
            .asset_provider
            .get_asset(
                AssetType::Obstruction,
                format!("{}/{}.json", obstruction_type, obstruction_id).as_str(),
            )
            .await?;
        let reader = BufReader::new(obstruction_meta_file);

        ObstructionMeta::from_json(reader, obstruction_type.clone()).map_err(|e| {
            AssetErr::AssetContentError(format!(
                "Error deserializing JSON for obstruction ID {} (type {}): {}",
                obstruction_id, obstruction_type, e
            ))
        })
    }

    async fn get_obstruction_raster(
        &self,
        obstruction_type: &ObstructionType,
        obstruction_id: ObstructionId,
    ) -> Result<ObstructionRaster, AssetErr> {
        let obstruction_raster_file = self
            .asset_provider
            .get_asset(
                AssetType::Obstruction,
                format!("{}/{}.tif", obstruction_type, obstruction_id).as_str(),
            )
            .await?;

        Ok(ObstructionRaster::read_from_tiff(
            obstruction_id,
            obstruction_raster_file,
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::backends::asset_fetcher::AssetType;
    use crate::providers::backends::fs_cache::AssetProvider;
    use crate::types::obstructions::ObstructionType;
    use crate::types::tiles::TileId;
    use std::fs;
    use std::fs::File;
    use std::path::PathBuf;
    use std::sync::Arc;
    use uuid::Uuid;

    struct MockAssetProvider {
        list_data: HashMap<String, Vec<String>>,
        asset_data: HashMap<(String, String), Vec<u8>>,
        temp_dir: PathBuf,
    }

    impl MockAssetProvider {
        fn new() -> Self {
            let temp_dir = std::env::temp_dir().join(format!("fresnel_mock_{}", Uuid::new_v4()));
            fs::create_dir_all(&temp_dir).unwrap();
            Self {
                list_data: HashMap::new(),
                asset_data: HashMap::new(),
                temp_dir,
            }
        }
        fn with_list(mut self, asset_type: AssetType, ids: &[&str]) -> Self {
            self.list_data.insert(
                asset_type.as_ref().to_string(),
                ids.iter().map(|s| s.to_string()).collect(),
            );
            self
        }
        fn with_asset(mut self, asset_type: AssetType, id: &str, content: Vec<u8>) -> Self {
            self.asset_data
                .insert((asset_type.as_ref().to_string(), id.to_string()), content);
            self
        }
    }

    impl Drop for MockAssetProvider {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.temp_dir);
        }
    }

    #[async_trait]
    impl AssetProvider for MockAssetProvider {
        async fn get_asset(&self, asset_type: AssetType, asset_id: &str) -> Result<File, AssetErr> {
            let key = (asset_type.as_ref().to_string(), asset_id.to_string());
            match self.asset_data.get(&key) {
                Some(content) => {
                    let safe = asset_id.replace(['/', '\\'], "_");
                    let path = self.temp_dir.join(format!("{}_{}", safe, Uuid::new_v4()));
                    fs::write(&path, content).unwrap();
                    Ok(File::open(&path).unwrap())
                }
                None => Err(AssetErr::AssetNotFound(format!(
                    "{}/{} not in mock",
                    asset_type, asset_id
                ))),
            }
        }
        async fn list_assets_of_type(
            &self,
            asset_type: AssetType,
        ) -> Result<Vec<String>, AssetErr> {
            Ok(self
                .list_data
                .get(asset_type.as_ref())
                .cloned()
                .unwrap_or_default())
        }
        fn get_local_asset_path(&self, _: AssetType, asset_id: &str) -> PathBuf {
            self.temp_dir.join(asset_id)
        }
    }

    fn arc(mock: MockAssetProvider) -> Arc<dyn AssetProvider + Send + Sync> {
        Arc::new(mock)
    }

    fn index_json(tile_str: &str, ids: &[Uuid]) -> Vec<u8> {
        let ids_str = ids
            .iter()
            .map(|u| format!("\"{}\"", u))
            .collect::<Vec<_>>()
            .join(",");
        format!(r#"{{"{}":[{}]}}"#, tile_str, ids_str).into_bytes()
    }

    #[tokio::test]
    async fn new_with_no_index_assets_produces_empty_index() {
        let provider = CachingObstructionProvider::new(arc(MockAssetProvider::new()))
            .await
            .unwrap();
        let tile = TileId::parse("982182_00").unwrap();
        assert!(
            provider
                .get_obstruction_ids_for_tile(tile)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn new_loads_valid_index_json() {
        let id = Uuid::new_v4();
        let mock = MockAssetProvider::new()
            .with_list(
                AssetType::ObstructionIndex,
                &["new_construction_footprints.json"],
            )
            .with_asset(
                AssetType::ObstructionIndex,
                "new_construction_footprints.json",
                index_json("982182_00", &[id]),
            );

        let provider = CachingObstructionProvider::new(arc(mock)).await.unwrap();
        let tile = TileId::parse("982182_00").unwrap();
        let result = provider.get_obstruction_ids_for_tile(tile).await.unwrap();

        let t = ObstructionType::parse("new_construction_footprints").unwrap();
        assert_eq!(result[&t], vec![id]);
    }

    #[tokio::test]
    async fn new_returns_asset_content_error_on_malformed_json() {
        let mock = MockAssetProvider::new()
            .with_list(
                AssetType::ObstructionIndex,
                &["new_construction_footprints.json"],
            )
            .with_asset(
                AssetType::ObstructionIndex,
                "new_construction_footprints.json",
                b"not json".to_vec(),
            );

        let result = CachingObstructionProvider::new(arc(mock)).await;
        assert!(matches!(result, Err(AssetErr::AssetContentError(_))));
    }

    #[tokio::test]
    async fn get_ids_returns_empty_for_unknown_tile() {
        let id = Uuid::new_v4();
        let mock = MockAssetProvider::new()
            .with_list(
                AssetType::ObstructionIndex,
                &["new_construction_footprints.json"],
            )
            .with_asset(
                AssetType::ObstructionIndex,
                "new_construction_footprints.json",
                index_json("982182_00", &[id]),
            );

        let provider = CachingObstructionProvider::new(arc(mock)).await.unwrap();
        let other = TileId::parse("990200_23").unwrap();
        assert!(
            provider
                .get_obstruction_ids_for_tile(other)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn get_ids_returns_all_matching_obstruction_types() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let tile_str = "982182_00";
        let mock = MockAssetProvider::new()
            .with_list(
                AssetType::ObstructionIndex,
                &["new_construction_footprints.json", "active_permits.json"],
            )
            .with_asset(
                AssetType::ObstructionIndex,
                "new_construction_footprints.json",
                index_json(tile_str, &[id1]),
            )
            .with_asset(
                AssetType::ObstructionIndex,
                "active_permits.json",
                index_json(tile_str, &[id2]),
            );

        let provider = CachingObstructionProvider::new(arc(mock)).await.unwrap();
        let tile = TileId::parse(tile_str).unwrap();
        let result = provider.get_obstruction_ids_for_tile(tile).await.unwrap();

        assert_eq!(
            result[&ObstructionType::parse("new_construction_footprints").unwrap()],
            vec![id1]
        );
        assert_eq!(
            result[&ObstructionType::parse("active_permits").unwrap()],
            vec![id2]
        );
    }

    #[tokio::test]
    async fn get_ids_tile_with_multiple_obstructions_same_type() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let tile_str = "982182_00";
        let mock = MockAssetProvider::new()
            .with_list(
                AssetType::ObstructionIndex,
                &["new_construction_footprints.json"],
            )
            .with_asset(
                AssetType::ObstructionIndex,
                "new_construction_footprints.json",
                index_json(tile_str, &[id1, id2]),
            );

        let provider = CachingObstructionProvider::new(arc(mock)).await.unwrap();
        let tile = TileId::parse(tile_str).unwrap();
        let result = provider.get_obstruction_ids_for_tile(tile).await.unwrap();

        let ids = &result[&ObstructionType::parse("new_construction_footprints").unwrap()];
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    #[tokio::test]
    async fn get_meta_propagates_asset_not_found() {
        let provider = CachingObstructionProvider::new(arc(MockAssetProvider::new()))
            .await
            .unwrap();
        let type_ = ObstructionType::NewConstructionFootprints;
        let id = Uuid::new_v4();
        let err = provider.get_obstruction_meta(&type_, id).await.unwrap_err();
        assert!(matches!(err, AssetErr::AssetNotFound(_)));
    }

    #[tokio::test]
    async fn get_meta_fetches_correct_path() {
        let type_ = ObstructionType::parse("new_construction_footprints").unwrap();
        let id = Uuid::new_v4();
        let expected_asset_id = format!("new_construction_footprints/{}.json", id);

        let mock = MockAssetProvider::new().with_asset(
            AssetType::Obstruction,
            &expected_asset_id,
            b"bad json".to_vec(),
        );

        let provider = CachingObstructionProvider::new(arc(mock)).await.unwrap();
        // The call should reach the asset (not return NotFound) and fail on JSON parsing
        let err = provider.get_obstruction_meta(&type_, id).await.unwrap_err();
        assert!(
            matches!(err, AssetErr::AssetContentError(_)),
            "expected AssetContentError (bad json), got {:?}",
            err
        );
    }
    #[tokio::test]
    async fn get_meta_reads_type_from_path_not_file() {
        let type_ = ObstructionType::parse("new_construction_footprints").unwrap();
        let id = Uuid::new_v4();
        let expected_asset_id = format!("new_construction_footprints/{}.json", id);

        let obstruction_metadata_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            "tests/resources/new_construction_footprints/83167e5c-c108-4d85-905c-6dc3224cc367.json",
        );

        let obstruction_meta_vec: Vec<u8> = fs::read(obstruction_metadata_path).unwrap();

        let mock = MockAssetProvider::new().with_asset(
            AssetType::Obstruction,
            &expected_asset_id,
            obstruction_meta_vec,
        );

        let provider = CachingObstructionProvider::new(arc(mock)).await.unwrap();
        let obstruction = provider.get_obstruction_meta(&type_, id).await.unwrap();
        assert_eq!(*obstruction.type_(), type_);
    }

    #[tokio::test]
    async fn get_meta_returns_content_error_on_bad_json() {
        let type_ = ObstructionType::parse("new_construction_footprints").unwrap();
        let id = Uuid::new_v4();
        let asset_id = format!("new_construction_footprints/{}.json", id);
        let mock = MockAssetProvider::new().with_asset(
            AssetType::Obstruction,
            &asset_id,
            b"{{invalid}}".to_vec(),
        );

        let provider = CachingObstructionProvider::new(arc(mock)).await.unwrap();
        let err = provider.get_obstruction_meta(&type_, id).await.unwrap_err();
        assert!(matches!(err, AssetErr::AssetContentError(_)));
    }

    #[tokio::test]
    async fn get_raster_propagates_asset_not_found() {
        let provider = CachingObstructionProvider::new(arc(MockAssetProvider::new()))
            .await
            .unwrap();
        let type_ = ObstructionType::parse("new_construction_footprints").unwrap();
        let id = Uuid::new_v4();
        let err = provider
            .get_obstruction_raster(&type_, id)
            .await
            .unwrap_err();
        assert!(matches!(err, AssetErr::AssetNotFound(_)));
    }

    #[tokio::test]
    async fn get_raster_fetches_correct_path() {
        let type_ = ObstructionType::parse("new_construction_footprints").unwrap();
        let id = Uuid::new_v4();
        let expected_asset_id = format!("new_construction_footprints/{}.tif", id);

        let mock = MockAssetProvider::new().with_asset(
            AssetType::Obstruction,
            &expected_asset_id,
            b"not a tiff".to_vec(),
        );

        let provider = CachingObstructionProvider::new(arc(mock)).await.unwrap();
        // Reaches the asset provider (not NotFound) and fails on TIFF parsing
        let err = provider
            .get_obstruction_raster(&type_, id)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AssetErr::AssetContentError(_)),
            "expected AssetContentError (bad tiff), got {:?}",
            err
        );
    }
}
