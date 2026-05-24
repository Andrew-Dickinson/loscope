use std::collections::HashMap;
use std::io::{BufReader, Read};
use std::sync::Arc;
use derive_new::new;
use crate::providers::backends::asset_fetcher::AssetType;
use crate::providers::backends::fs_cache::AssetProvider;
use crate::types::errors::AssetErr;
use crate::types::obstructions::{ObstructionId, ObstructionMeta, ObstructionRaster, ObstructionType};
use crate::types::tiles::TileId;

#[async_trait]
pub trait ObstructionProvider {
    async fn get_obstruction_ids_for_tile(&self, tile_id: TileId) -> Result<HashMap<ObstructionType, Vec<ObstructionId>>, AssetErr>;
    async fn get_obstruction_meta(&self, obstruction_type: &ObstructionType, obstruction_id: ObstructionId) -> Result<ObstructionMeta, AssetErr>;
    async fn get_obstruction_raster(&self, obstruction_type: &ObstructionType, obstruction_id: ObstructionId) -> Result<ObstructionRaster, AssetErr>;
}


pub struct CachingObstructionProvider  {
    asset_provider: Arc<dyn AssetProvider + Send + Sync>,
    obstruction_index: HashMap<ObstructionType, HashMap<TileId, Vec<ObstructionId>>>,
}

impl CachingObstructionProvider {
    pub async fn new(asset_provider: Arc<dyn AssetProvider + Send + Sync>) -> Result<Self, AssetErr> {
        let mut obstruction_index = HashMap::new();

        for obstruction_type in asset_provider.list_assets_of_type(AssetType::ObstructionIndex).await? {
            let index_file = asset_provider.get_asset(AssetType::ObstructionIndex, &obstruction_type).await?;
            let reader = BufReader::new(index_file);

            let index: HashMap<TileId, Vec<ObstructionId>> = serde_json::from_reader(reader)
                .map_err(|e| AssetErr::AssetContentError(
                    format!(
                        "Error deserializing JSON index for obstruction type {}: {}",
                        obstruction_type, e
                    )
                ))?;

            let Ok(obstruction_type) = ObstructionType::parse(&obstruction_type) else { continue };
            obstruction_index.insert(obstruction_type, index);
        }

        Ok(Self { asset_provider, obstruction_index })
    }
}

#[async_trait]
impl ObstructionProvider for CachingObstructionProvider {
    async fn get_obstruction_ids_for_tile(&self, tile_id: TileId) -> Result<HashMap<ObstructionType, Vec<ObstructionId>>, AssetErr> {
        let mut output: HashMap<ObstructionType, Vec<ObstructionId>> = HashMap::new();
        self.obstruction_index.iter().for_each(|(type_, tile_map)| {
            if let Some(obstructions) = tile_map.get(&tile_id) {
                output.insert(type_.clone(), obstructions.clone());
            }
        });

        Ok(output)
    }

    async fn get_obstruction_meta(&self, obstruction_type: &ObstructionType, obstruction_id: ObstructionId) -> Result<ObstructionMeta, AssetErr> {
        let obstruction_meta_file = self.asset_provider.get_asset(
            AssetType::Obstruction,
            format!("{}/{}.json", obstruction_type, obstruction_id).as_str()
        ).await?;
        let reader = BufReader::new(obstruction_meta_file);

        let obstruction_meta: ObstructionMeta = serde_json::from_reader(reader)
            .map_err(|e| AssetErr::AssetContentError(
                format!(
                    "Error deserializing JSON for obstruction ID {} (type {}): {}",
                    obstruction_id, obstruction_type, e
                )
            ))?;

        Ok(obstruction_meta)
    }

    async fn get_obstruction_raster(&self, obstruction_type: &ObstructionType, obstruction_id: ObstructionId) -> Result<ObstructionRaster, AssetErr> {
        let obstruction_raster_file = self.asset_provider.get_asset(
            AssetType::Obstruction,
            format!("{}/{}.tif", obstruction_type, obstruction_id).as_str()
        ).await?;

        Ok(ObstructionRaster::read_from_tiff(obstruction_id, obstruction_raster_file)?)
    }
}