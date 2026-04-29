use std::io::{Read};
use derive_new::new;
use image::{DynamicImage};
use crate::util::openjpg2k::decode_jp2_region;
use crate::providers::backends::asset_fetcher::{AssetType};
use crate::providers::backends::fs_cache::{AssetProvider};
use crate::types::errors::AssetErr;
use crate::types::tiles::{TileId, LAS_TILE_SIDE_LENGTH_USFT};

const ORTHO_IMAGE_SIZE_PIXELS: u16 = 5000;
const ORTHO_SCALE_PX_PER_USFT: u8 = 2;

#[async_trait]
pub trait OrthoProvider {
    async fn get_ortho(&self, tile_id: &TileId) -> Result<DynamicImage, AssetErr>;
}


#[derive(new)]
pub struct CachingOrthoProvider  {
    asset_provider: Box<dyn AssetProvider + Send + Sync>,
}

#[async_trait]
impl OrthoProvider for CachingOrthoProvider {
    async fn get_ortho(&self, tile_id: &TileId) -> Result<DynamicImage, AssetErr> {
        let fname = tile_id.las_tile_id().ortho_fname();
        let mut asset_handle = self.asset_provider.get_asset(&AssetType::OrthoImage, &fname).await?;


        let asset_size = asset_handle.metadata().or_else(
            |read_err| Err(AssetErr::AssetContentError(
                format!("Unable to read metadata from asset {fname}: {read_err}")
            )))?.len();

        // Safety: This unwrap() would only throw if asset_size > max usize, which could only happen
        // when usize is < u64, and we are trying to load an asset that wouldn't fit into memory,
        // in which case a panic is justified (since the Vec wouldn't be able to alloc anyway)
        let mut asset_buf: Vec<u8> = Vec::with_capacity(asset_size.try_into().unwrap());
        asset_handle.read_to_end(&mut asset_buf).or_else(
            |read_err| Err(AssetErr::AssetContentError(
                    format!("Unable to read content from asset {fname}: {read_err}")
                )))?;

        let bounds = to_ortho_bounds(tile_id.subgrid_id().relative_bounds());
        let rgba_img = decode_jp2_region(
            asset_buf,
            bounds.0 as i32,
            bounds.1 as i32,
            bounds.2 as i32,
            bounds.3 as i32
        ).or_else(
            |read_err| Err(AssetErr::AssetContentError(
                format!("Unable to read content from asset {fname}: {read_err}")
            )))?;

        Ok(DynamicImage::ImageRgba8(rgba_img))
    }
}

fn to_ortho_bounds(usft_sw_rel_bounds: (u16, u16, u16, u16)) -> (u16, u16, u16, u16){
    let top_left = (
        usft_sw_rel_bounds.0 * ORTHO_SCALE_PX_PER_USFT as u16,
        // Convert SW (bottom right) offset to top left instead. Double subtract is
        // to both move the coordinate reference point to the top left of the LAS tile,
        // and also refer to the top left corner of the subgrid square
        // (relative_bounds() returns the SW corner of the subgrid tile, relative to the
        // SW corner of the LAS tile)
        (LAS_TILE_SIDE_LENGTH_USFT - usft_sw_rel_bounds.3 - usft_sw_rel_bounds.1)
            * ORTHO_SCALE_PX_PER_USFT as u16
    );

    // Convert from x,y,w,h to x0,y0,x1,y1
    (
        top_left.0,
        top_left.1,
        top_left.0 + usft_sw_rel_bounds.2 * ORTHO_SCALE_PX_PER_USFT as u16,
        top_left.1 + usft_sw_rel_bounds.3 * ORTHO_SCALE_PX_PER_USFT as u16
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use std::path::PathBuf;
    use test_temp_dir::test_temp_dir;

    struct MockAssetProvider {
        result: Result<PathBuf, AssetErr>,
    }

    impl MockAssetProvider {
        fn returning_file(path: PathBuf) -> Self {
            Self { result: Ok(path) }
        }

        fn returning_err(err: AssetErr) -> Self {
            Self { result: Err(err) }
        }
    }

    #[async_trait]
    impl AssetProvider for MockAssetProvider {
        async fn get_asset(&self, _: &AssetType, _: &str) -> Result<File, AssetErr> {
            match &self.result {
                Ok(path) => File::open(path).map_err(|e| AssetErr::LocalFileSystemError(e.to_string())),
                Err(AssetErr::AssetNotFound(msg)) => Err(AssetErr::AssetNotFound(msg.clone())),
                Err(e) => Err(AssetErr::LocalFileSystemError(format!("{e:?}"))),
            }
        }
    }

    // --- to_ortho_bounds ---

    #[test]
    fn ortho_bounds_sw_subgrid() {
        // Subgrid (0,0): SW corner of LAS tile maps to bottom-left of image → top pixel row 4000
        assert_eq!(to_ortho_bounds((0, 0, 500, 500)), (0, 4000, 1000, 5000));
    }

    #[test]
    fn ortho_bounds_ne_subgrid() {
        // Subgrid (4,4): NE corner → top-left pixel origin of image
        assert_eq!(to_ortho_bounds((2000, 2000, 500, 500)), (4000, 0, 5000, 1000));
    }

    #[test]
    fn ortho_bounds_middle_subgrid() {
        // Subgrid (2,3): x0=1000*2=2000, y_tl=(2500-500-1500)*2=1000
        assert_eq!(to_ortho_bounds((1000, 1500, 500, 500)), (2000, 1000, 3000, 2000));
    }

    #[test]
    fn ortho_bounds_x_scales_independently() {
        // Moving one step east shifts x0/x1 by 1000 px, y unchanged
        let (x0_a, y0_a, x1_a, y1_a) = to_ortho_bounds((0, 0, 500, 500));
        let (x0_b, y0_b, x1_b, y1_b) = to_ortho_bounds((500, 0, 500, 500));
        assert_eq!(x0_b - x0_a, 1000);
        assert_eq!(x1_b - x1_a, 1000);
        assert_eq!(y0_a, y0_b);
        assert_eq!(y1_a, y1_b);
    }

    #[test]
    fn ortho_bounds_y_flipped() {
        // Moving one step north (higher y_sw) shifts y0/y1 upward (lower pixel value)
        let (_, y0_a, _, y1_a) = to_ortho_bounds((0, 0, 500, 500));
        let (_, y0_b, _, y1_b) = to_ortho_bounds((0, 500, 500, 500));
        assert_eq!(y0_a - y0_b, 1000);
        assert_eq!(y1_a - y1_b, 1000);
    }

    #[test]
    fn ortho_bounds_output_size_matches_input() {
        // Output pixel region should be w*2 by h*2 for any valid subgrid
        for x in 0..5u16 {
            for y in 0..5u16 {
                let usft = (x * 500, y * 500, 500u16, 500u16);
                let (x0, y0, x1, y1) = to_ortho_bounds(usft);
                assert_eq!(x1 - x0, 1000);
                assert_eq!(y1 - y0, 1000);
            }
        }
    }

    // --- get_ortho ---

    // 002205.jp2 -> TileId "002205_00": easting=1002 (id=2), northing=205 -> fname "002205.jp2"
    fn tile_002205() -> TileId {
        TileId::parse("002205_00").unwrap()
    }

    #[tokio::test]
    async fn get_ortho_returns_image_for_valid_jp2() {
        let jp2_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/resources/002205.jp2");

        let provider = CachingOrthoProvider::new(
            Box::new(MockAssetProvider::returning_file(jp2_path))
        );
        let result = provider.get_ortho(&tile_002205()).await;
        assert!(result.is_ok(), "expected Ok, got {result:?}");
        let result = result.unwrap();
        assert_eq!(result.width(), 1000);
        assert_eq!(result.height(), 1000);
    }

    #[tokio::test]
    async fn get_ortho_propagates_asset_not_found() {
        let provider = CachingOrthoProvider::new(
            Box::new(
                MockAssetProvider::returning_err(AssetErr::AssetNotFound("mock: not found".into()))
            )
        );
        let result = provider.get_ortho(&tile_002205()).await;
        assert!(matches!(result, Err(AssetErr::AssetNotFound(_))));
    }

    #[tokio::test]
    async fn get_ortho_returns_content_error_for_corrupt_file() {
        let temp_dir = test_temp_dir!();
        let bad_path = temp_dir.as_path_untracked().join("bad.jp2");
        File::create(&bad_path).unwrap().write_all(b"not-an-image").unwrap();

        let provider = CachingOrthoProvider::new(
            Box::new(
                MockAssetProvider::returning_file(bad_path)
            )
        );
        let result = provider.get_ortho(&tile_002205()).await;
        assert!(matches!(result, Err(AssetErr::AssetContentError(_))));
    }
}

