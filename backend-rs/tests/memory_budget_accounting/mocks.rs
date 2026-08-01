// Mock providers that plug into the *real* allocation-heavy code (evaluate_points,
// RooftopHeightMapFactory::create, CachingOrthoProvider::get_ortho, ...) so property tests drive
// actual production allocation logic rather than re-implementing/guessing at it. Only the I/O
// edges (network/disk fetch) are faked; everything past that boundary is real code.

use async_trait::async_trait;
use loscope::providers::backends::asset_fetcher::AssetType;
use loscope::providers::backends::fs_cache::AssetProvider;
use loscope::providers::elevation_tile_provider::{ElevationTile, ElevationTileProvider};
use loscope::providers::footprint_provider::FootprintProvider;
use loscope::providers::obstruction_provider::ObstructionProvider;
use loscope::types::coords::NYSCoords2;
use loscope::types::errors::AssetErr;
use loscope::types::obstructions::{ObstructionId, ObstructionMeta, ObstructionRaster, ObstructionType};
use loscope::types::tiles::{SUBGRID_TILE_SIDE_LENGTH_USFT, TileId};
use ndarray::Array2;
use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::sync::Mutex;
use uuid::Uuid;

/// Serves the same flat-elevation 500x500 tile for any `TileId` requested, mirroring the shape of
/// real `CachingElevationTileProvider` output without touching disk.
pub struct FlatElevationProvider {
    pub value: u16,
}

#[async_trait]
impl ElevationTileProvider for FlatElevationProvider {
    async fn get_elevation_tile(&self, tile_id: TileId) -> Result<ElevationTile, AssetErr> {
        let side = usize::from(SUBGRID_TILE_SIDE_LENGTH_USFT);
        Ok(ElevationTile::new(tile_id, Array2::from_elem((side, side), self.value)))
    }
}

/// Reports zero obstructions for every tile. Used to isolate the terrain/zone allocation math
/// from obstruction accounting.
pub struct NoObstructions;

#[async_trait]
impl ObstructionProvider for NoObstructions {
    async fn get_obstruction_ids_for_tile(
        &self,
        _tile_id: TileId,
    ) -> Result<HashMap<ObstructionType, Vec<ObstructionId>>, AssetErr> {
        Ok(HashMap::new())
    }

    async fn get_obstruction_meta(
        &self,
        _obstruction_type: &ObstructionType,
        _obstruction_id: ObstructionId,
    ) -> Result<ObstructionMeta, AssetErr> {
        unreachable!("NoObstructions never advertises any obstruction ids")
    }

    async fn get_obstruction_raster(
        &self,
        _obstruction_type: &ObstructionType,
        _obstruction_id: ObstructionId,
    ) -> Result<ObstructionRaster, AssetErr> {
        unreachable!("NoObstructions never advertises any obstruction ids")
    }
}

/// Synthesizes `obstructions_per_tile` distinct obstructions, each with a `raster_w x raster_h`
/// u16 heightmap raster, for every distinct tile it's asked about (lazily, memoized, so the two
/// sequential passes `evaluate_points` makes over the same tile set see a consistent count).
///
/// Placement (`sw_offset`) is fixed at the NYS origin regardless of the requesting tile: this is
/// safe because `bilt_impl` (analysis/tiles.rs) is designed to no-op gracefully on
/// non-overlapping source/zone placement rather than panic (see its early-return paths for
/// negative or empty overlap ranges) — and we only care about the raster's *resident memory* here,
/// not the correctness of the compositing.
pub struct DenseObstructionProvider {
    obstructions_per_tile: usize,
    raster_w: usize,
    raster_h: usize,
    generated: Mutex<HashMap<TileId, Vec<ObstructionId>>>,
    dims: Mutex<HashMap<ObstructionId, (usize, usize)>>,
}

impl DenseObstructionProvider {
    pub fn new(obstructions_per_tile: usize, raster_w: usize, raster_h: usize) -> Self {
        Self {
            obstructions_per_tile,
            raster_w,
            raster_h,
            generated: Mutex::new(HashMap::new()),
            dims: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl ObstructionProvider for DenseObstructionProvider {
    async fn get_obstruction_ids_for_tile(
        &self,
        tile_id: TileId,
    ) -> Result<HashMap<ObstructionType, Vec<ObstructionId>>, AssetErr> {
        let mut generated = self.generated.lock().unwrap();
        let ids = generated
            .entry(tile_id)
            .or_insert_with(|| {
                let mut dims = self.dims.lock().unwrap();
                (0..self.obstructions_per_tile)
                    .map(|_| {
                        let id = Uuid::new_v4();
                        dims.insert(id, (self.raster_w, self.raster_h));
                        id
                    })
                    .collect()
            })
            .clone();
        let mut out = HashMap::new();
        if !ids.is_empty() {
            out.insert(ObstructionType::ActivePermits, ids);
        }
        Ok(out)
    }

    async fn get_obstruction_meta(
        &self,
        obstruction_type: &ObstructionType,
        obstruction_id: ObstructionId,
    ) -> Result<ObstructionMeta, AssetErr> {
        let dims = self.dims.lock().unwrap();
        let _ = dims
            .get(&obstruction_id)
            .ok_or_else(|| AssetErr::AssetNotFound("unknown synthetic obstruction id".into()))?;
        Ok(ObstructionMeta::new(
            obstruction_id,
            obstruction_type.clone(),
            HashMap::new(),
            NYSCoords2::new(0.0, 0.0),
            vec![],
            None,
        ))
    }

    async fn get_obstruction_raster(
        &self,
        _obstruction_type: &ObstructionType,
        obstruction_id: ObstructionId,
    ) -> Result<ObstructionRaster, AssetErr> {
        let (w, h) = *self
            .dims
            .lock()
            .unwrap()
            .get(&obstruction_id)
            .ok_or_else(|| AssetErr::AssetNotFound("unknown synthetic obstruction id".into()))?;
        Ok(ObstructionRaster::new(Array2::<u16>::zeros((w, h))))
    }
}

/// A `FootprintProvider` that always returns the same fixed polygon, regardless of the requested
/// `BINId`.
pub struct FixedFootprintProvider {
    pub polygon: geo::Polygon,
}

#[async_trait]
impl FootprintProvider for FixedFootprintProvider {
    async fn get_footprint(&self, _bin_id: loscope::building::bin_id::BINId) -> Result<geo::Polygon, AssetErr> {
        Ok(self.polygon.clone())
    }
}

/// An `AssetProvider` that always serves the same file from disk, regardless of the requested
/// asset type/id. Used to drive `CachingOrthoProvider` (and friends) with either a real fixture or
/// a synthetic file of a controlled size.
pub struct FixedFileAssetProvider {
    pub path: PathBuf,
}

#[async_trait]
impl AssetProvider for FixedFileAssetProvider {
    fn get_local_asset_path(&self, _asset_type: AssetType, _asset_id: &str) -> PathBuf {
        self.path.clone()
    }

    async fn get_asset(&self, _asset_type: AssetType, _asset_id: &str) -> Result<File, AssetErr> {
        File::open(&self.path).map_err(|e| AssetErr::LocalFileSystemError(e.to_string()))
    }

    async fn list_assets_of_type(&self, _asset_type: AssetType) -> Result<Vec<String>, AssetErr> {
        unreachable!("list_assets_of_type not exercised by these tests")
    }
}

/// Writes `size_bytes` of arbitrary content to a fresh temp file and returns a path to it. Used to
/// measure the raw-file-buffer allocation in `CachingOrthoProvider::get_ortho` independent of
/// whether the content is a valid JP2 (the buffer is allocated and fully read *before* decoding is
/// attempted, so decode failure doesn't prevent the allocation from happening).
pub fn write_garbage_file(size_bytes: usize) -> tempfile::TempPath {
    use std::io::Write;
    let mut f = tempfile::NamedTempFile::new().expect("failed to create temp file");
    // Non-zero, non-repeating-enough-to-compress-trivially content, written in chunks to avoid a
    // second full-size buffer just to build the content.
    let chunk = vec![0xABu8; 64 * 1024];
    let mut remaining = size_bytes;
    while remaining > 0 {
        let n = remaining.min(chunk.len());
        f.write_all(&chunk[..n]).expect("failed to write temp file contents");
        remaining -= n;
    }
    f.flush().expect("failed to flush temp file");
    f.into_temp_path()
}
