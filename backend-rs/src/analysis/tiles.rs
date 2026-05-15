use derive_getters::Getters;
use derive_new::new;
use crate::analysis::fresnel_zone::FresnelZone;
use crate::providers::elevation_tile_provider::ElevationTileProvider;
use crate::types::coords::NYSCoords2;
use crate::types::stairstep::StairStepGrid;
use crate::types::tiles::TileId;


pub(crate) type TerrainGrid = StairStepGrid<u16>;

pub fn get_intersecting_tiles(fresnel_zone: &FresnelZone) -> Vec<TileId> {
    todo!()
}

#[derive(new)]
pub struct TerrainFactory<'a> {
    tile_provider: &'a (dyn ElevationTileProvider + Sync + Send)
}

impl<'a> TerrainFactory<'a> {
    pub fn load_terrain_grid(&self, tile_ids: &Vec<TileId>, zone: &FresnelZone) -> TerrainGrid {
        // self.tile_provider.get_elevation_tile() ...
        todo!()
    }
}
