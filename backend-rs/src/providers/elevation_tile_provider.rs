use crate::providers::backends::asset_fetcher::AssetType;
use crate::providers::backends::fs_cache::AssetProvider;
use crate::types::errors::AssetErr;
use crate::types::tiles::{SUBGRID_TILE_SIDE_LENGTH_USFT, TileId};
use derive_getters::Getters;
use derive_new::new;
use ndarray::Array2;
use std::fs::File;
use std::io;
use std::io::{Seek, Write};
use std::sync::Arc;
use tiff::TiffError;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::encoder::{TiffEncoder, colortype};
use tokio_rusqlite::fallible_iterator::FallibleIterator;

#[derive(new, Debug, Getters)]
pub struct ElevationTile {
    id: TileId,
    // Values are in inches above the NY SP Long Island datum,
    // axes are [easting_local, northing_local] (add the sw corner coords from the
    // TileId to get the global position)
    elevation_inches: Array2<u16>,
}

impl ElevationTile {
    pub fn new_empty(id: TileId) -> ElevationTile {
        ElevationTile {
            id,
            elevation_inches: Array2::zeros((
                SUBGRID_TILE_SIDE_LENGTH_USFT.into(),
                SUBGRID_TILE_SIDE_LENGTH_USFT.into(),
            )),
        }
    }

    pub fn read_from_tiff(id: TileId, file: File) -> Result<ElevationTile, AssetErr> {
        let io = std::io::BufReader::new(file);

        // We would love to use a try here to scope the ?s but it's only available in nightly,
        // so a closure it is
        let inner = move || -> Result<Array2<u16>, Box<dyn std::error::Error>> {
            let mut reader = Decoder::new(io)?;
            let image_data = reader.read_image()?;
            let width: usize = reader.dimensions()?.0.try_into()?;
            let height: usize = reader.dimensions()?.1.try_into()?;

            let subgrid_tile_side_length_usft_usize: usize = SUBGRID_TILE_SIDE_LENGTH_USFT.into();

            if height != subgrid_tile_side_length_usft_usize
                || width != subgrid_tile_side_length_usft_usize
            {
                return Err(Box::new(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid tiff file size: ({width}, {height})"),
                )));
            }

            let colortype = reader.colortype()?;
            if colortype != tiff::ColorType::Gray(16) {
                return Err(Box::new(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid datatype: {:?}", colortype),
                )));
            }

            if let DecodingResult::U16(image_data) = image_data {
                Ok(Array2::from_shape_vec((height, width), image_data)
                    .or_else(|arr_err| Err(Box::new(arr_err)))?)
            } else {
                Err(Box::new(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid datatype: {:?}", image_data),
                )))
            }
        };

        let image_data = inner().or_else(|err| {
            Err(AssetErr::AssetContentError(format!(
                "Error parsing tile tiff for id {id}: {err}"
            )))
        })?;

        Ok(ElevationTile {
            id,
            elevation_inches: image_data,
        })
    }

    pub fn write_to_tiff<W: Write + Seek>(&self, mut writer: W) -> Result<(), TiffError> {
        let mut tiff = TiffEncoder::new(&mut writer)?;
        tiff.write_image::<colortype::Gray16>(
            SUBGRID_TILE_SIDE_LENGTH_USFT.into(),
            SUBGRID_TILE_SIDE_LENGTH_USFT.into(),
            // Safety: as_slice() is only None when elevation_inches is non-contiguous (impossible)
            // or in non-standard order (also maybe impossible?, but definitely against convention)
            &self.elevation_inches.as_slice().unwrap(),
        )?;
        Ok(())
    }
}

#[async_trait]
pub trait ElevationTileProvider {
    async fn get_elevation_tile(&self, tile_id: TileId) -> Result<ElevationTile, AssetErr>;
}

#[derive(new)]
pub struct CachingElevationTileProvider {
    asset_provider: Arc<dyn AssetProvider + Send + Sync>,
}

#[async_trait]
impl ElevationTileProvider for CachingElevationTileProvider {
    async fn get_elevation_tile(&self, tile_id: TileId) -> Result<ElevationTile, AssetErr> {
        let asset_id = &tile_id.tiff_fname();
        let asset_res = self
            .asset_provider
            .get_asset(AssetType::ElevationTile, asset_id)
            .await;

        match asset_res {
            Ok(asset) => ElevationTile::read_from_tiff(tile_id, asset),
            Err(err) => {
                if let AssetErr::AssetNotFound(_) = err {
                    // As an optimization, we chose not to store tiles which are all zeros, but analyses may request these
                    // TODO: Warn the user for the case where their analysis is trying to query a tile outside the city
                    //     (might require an extra lookup here to see if the tile ID falls outside some kind of index)
                    let path = self
                        .asset_provider
                        .get_local_asset_path(AssetType::ElevationTile, asset_id);
                    let tile = ElevationTile::new_empty(tile_id);
                    File::create(path.as_path())
                        .map_err(|err| err.to_string())
                        .and_then(|f| tile.write_to_tiff(f).map_err(|err| err.to_string()))
                        .or_else(|err| {
                            Err(AssetErr::LocalFileSystemError(format!(
                                "Error writing empty tile to {path:?}: {err}"
                            )))
                        })?;
                    Ok(tile)
                } else {
                    Err(err)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use test_temp_dir::test_temp_dir;
    use tiff::encoder::{TiffEncoder, colortype};

    fn sample_tile_id() -> TileId {
        TileId::parse("2235_13").unwrap()
    }

    fn sample_tif_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/resources/2235_13.tif")
    }

    fn write_gray16_tiff(file: &mut File, width: u32, height: u32, data: &[u16]) {
        TiffEncoder::new(file)
            .unwrap()
            .write_image::<colortype::Gray16>(width, height, data)
            .unwrap();
    }

    fn write_gray8_tiff(file: &mut File, width: u32, height: u32, data: &[u8]) {
        TiffEncoder::new(file)
            .unwrap()
            .write_image::<colortype::Gray8>(width, height, data)
            .unwrap();
    }

    // --- ElevationTile::new_empty ---

    #[test]
    fn new_empty_has_correct_dimensions() {
        let tile = ElevationTile::new_empty(sample_tile_id());
        let side = SUBGRID_TILE_SIDE_LENGTH_USFT as usize;
        assert_eq!(tile.elevation_inches.nrows(), side);
        assert_eq!(tile.elevation_inches.ncols(), side);
    }

    #[test]
    fn new_empty_all_zeros() {
        let tile = ElevationTile::new_empty(sample_tile_id());
        assert!(tile.elevation_inches.iter().all(|&v| v == 0));
    }

    // --- ElevationTile::read_from_tiff ---

    #[test]
    fn read_from_tiff_succeeds_on_valid_file() {
        let file = File::open(sample_tif_path()).unwrap();
        assert!(ElevationTile::read_from_tiff(sample_tile_id(), file).is_ok());
    }

    #[test]
    fn read_from_tiff_correct_dimensions() {
        let file = File::open(sample_tif_path()).unwrap();
        let tile = ElevationTile::read_from_tiff(sample_tile_id(), file).unwrap();
        let side = SUBGRID_TILE_SIDE_LENGTH_USFT as usize;
        assert_eq!(tile.elevation_inches.nrows(), side);
        assert_eq!(tile.elevation_inches.ncols(), side);
    }

    #[test]
    fn read_from_tiff_known_pixel_values() {
        // Values pre-computed from the raw TIFF. Row-major storage means
        // array[(r, c)] = raw_data[r * 500 + c].
        let file = File::open(sample_tif_path()).unwrap();
        let tile = ElevationTile::read_from_tiff(sample_tile_id(), file).unwrap();
        assert_eq!(tile.elevation_inches.get((0, 0)), Some(&210));
        assert_eq!(tile.elevation_inches.get((499, 499)), Some(&485));
        assert_eq!(tile.elevation_inches.get((0, 499)), Some(&113));
        assert_eq!(tile.elevation_inches.get((499, 0)), Some(&234));
        assert_eq!(tile.elevation_inches.get((250, 250)), Some(&108));
        assert_eq!(tile.elevation_inches.get((100, 200)), Some(&170));
    }

    #[test]
    fn read_from_tiff_wrong_dimensions_fails() {
        let temp = test_temp_dir!();
        let path = temp.as_path_untracked().join("small.tiff");
        write_gray16_tiff(&mut File::create(&path).unwrap(), 10, 10, &vec![0u16; 100]);

        let result = ElevationTile::read_from_tiff(sample_tile_id(), File::open(&path).unwrap());
        assert!(matches!(result, Err(AssetErr::AssetContentError(_))));
    }

    #[test]
    fn read_from_tiff_wrong_colortype_fails() {
        let temp = test_temp_dir!();
        let path = temp.as_path_untracked().join("gray8.tiff");
        let side = SUBGRID_TILE_SIDE_LENGTH_USFT as u32;
        write_gray8_tiff(
            &mut File::create(&path).unwrap(),
            side,
            side,
            &vec![0u8; (side * side) as usize],
        );

        let result = ElevationTile::read_from_tiff(sample_tile_id(), File::open(&path).unwrap());
        assert!(matches!(result, Err(AssetErr::AssetContentError(_))));
    }

    #[test]
    fn read_from_tiff_invalid_file_fails() {
        let temp = test_temp_dir!();
        let path = temp.as_path_untracked().join("garbage.tiff");
        File::create(&path)
            .unwrap()
            .write_all(b"not a tiff")
            .unwrap();

        let result = ElevationTile::read_from_tiff(sample_tile_id(), File::open(&path).unwrap());
        assert!(matches!(result, Err(AssetErr::AssetContentError(_))));
    }

    // --- ElevationTile::write_to_tiff / round-trip ---

    #[test]
    fn write_to_tiff_roundtrip_empty() {
        let temp = test_temp_dir!();
        let path = temp.as_path_untracked().join("out.tiff");

        let original = ElevationTile::new_empty(sample_tile_id());
        original
            .write_to_tiff(File::create(&path).unwrap())
            .unwrap();

        let restored =
            ElevationTile::read_from_tiff(sample_tile_id(), File::open(&path).unwrap()).unwrap();
        assert!(restored.elevation_inches.iter().all(|&v| v == 0));
    }

    #[test]
    fn write_to_tiff_roundtrip_sample_file() {
        let temp = test_temp_dir!();
        let path = temp.as_path_untracked().join("roundtrip.tif");

        let original =
            ElevationTile::read_from_tiff(sample_tile_id(), File::open(sample_tif_path()).unwrap())
                .unwrap();
        original
            .write_to_tiff(File::create(&path).unwrap())
            .unwrap();

        let restored =
            ElevationTile::read_from_tiff(sample_tile_id(), File::open(&path).unwrap()).unwrap();

        let orig_vals: Vec<u16> = original.elevation_inches.iter().copied().collect();
        let rest_vals: Vec<u16> = restored.elevation_inches.iter().copied().collect();
        assert_eq!(orig_vals, rest_vals);
    }

    // --- CachingElevationTileProvider ---

    struct MockAssetProvider {
        get_asset_result: Result<std::path::PathBuf, AssetErr>,
        local_asset_path: std::path::PathBuf,
    }

    impl MockAssetProvider {
        fn returning_file(path: std::path::PathBuf) -> Self {
            Self {
                get_asset_result: Ok(path.clone()),
                local_asset_path: path,
            }
        }

        fn returning_not_found(local_path: std::path::PathBuf) -> Self {
            Self {
                get_asset_result: Err(AssetErr::AssetNotFound("mock: not found".into())),
                local_asset_path: local_path,
            }
        }

        fn returning_err(err: AssetErr) -> Self {
            Self {
                get_asset_result: Err(err),
                local_asset_path: std::path::PathBuf::new(),
            }
        }
    }

    #[async_trait]
    impl AssetProvider for MockAssetProvider {
        fn get_local_asset_path(&self, _: AssetType, _: &str) -> std::path::PathBuf {
            self.local_asset_path.clone()
        }

        async fn get_asset(&self, _: AssetType, _: &str) -> Result<File, AssetErr> {
            match &self.get_asset_result {
                Ok(path) => {
                    File::open(path).map_err(|e| AssetErr::LocalFileSystemError(e.to_string()))
                }
                Err(AssetErr::AssetNotFound(msg)) => Err(AssetErr::AssetNotFound(msg.clone())),
                Err(e) => Err(AssetErr::LocalFileSystemError(format!("{e:?}"))),
            }
        }

        async fn list_assets_of_type(
            &self,
            _asset_type: AssetType,
        ) -> Result<Vec<String>, AssetErr> {
            panic!("MockAssetProvider::list_assets_of_type");
        }
    }

    #[tokio::test]
    async fn get_elevation_tile_returns_tile_for_valid_asset() {
        let provider = CachingElevationTileProvider::new(Arc::new(
            MockAssetProvider::returning_file(sample_tif_path()),
        ));
        let result = provider.get_elevation_tile(sample_tile_id()).await;
        assert!(result.is_ok(), "expected Ok, got {result:?}");
        let tile = result.unwrap();
        let side = SUBGRID_TILE_SIDE_LENGTH_USFT as usize;
        assert_eq!(tile.elevation_inches.nrows(), side);
        assert_eq!(tile.elevation_inches.ncols(), side);
    }

    #[tokio::test]
    async fn get_elevation_tile_asset_content_error_for_corrupt_file() {
        let temp = test_temp_dir!();
        let path = temp.as_path_untracked().join("bad.tiff");
        File::create(&path)
            .unwrap()
            .write_all(b"not a tiff")
            .unwrap();

        let provider =
            CachingElevationTileProvider::new(Arc::new(MockAssetProvider::returning_file(path)));
        let result = provider.get_elevation_tile(sample_tile_id()).await;
        assert!(matches!(result, Err(AssetErr::AssetContentError(_))));
    }

    #[tokio::test]
    async fn get_elevation_tile_asset_not_found_creates_empty_tile() {
        let temp = test_temp_dir!();
        let cache_path = temp.as_path_untracked().join("2235_13.tiff");

        let provider = CachingElevationTileProvider::new(Arc::new(
            MockAssetProvider::returning_not_found(cache_path.clone()),
        ));
        let result = provider.get_elevation_tile(sample_tile_id()).await;

        assert!(result.is_ok(), "expected Ok, got {result:?}");
        let tile = result.unwrap();
        assert!(tile.elevation_inches.iter().all(|&v| v == 0));
        assert!(
            cache_path.exists(),
            "empty tile should have been written to the cache path"
        );
    }

    #[tokio::test]
    async fn get_elevation_tile_asset_not_found_local_fs_error_when_path_unwritable() {
        // Parent directory does not exist, so File::create will fail
        let bad_path = std::path::PathBuf::from("/nonexistent/dir/2235_13.tiff");

        let provider = CachingElevationTileProvider::new(Arc::new(
            MockAssetProvider::returning_not_found(bad_path),
        ));
        let result = provider.get_elevation_tile(sample_tile_id()).await;
        assert!(matches!(result, Err(AssetErr::LocalFileSystemError(_))));
    }

    #[tokio::test]
    async fn get_elevation_tile_propagates_non_not_found_errors() {
        let provider =
            CachingElevationTileProvider::new(Arc::new(MockAssetProvider::returning_err(
                AssetErr::AssetDownloadError("network failure".into()),
            )));
        let result = provider.get_elevation_tile(sample_tile_id()).await;
        assert!(matches!(result, Err(AssetErr::LocalFileSystemError(_))));
    }
}
