use crate::building::bin_id::BINId;
use crate::providers::backends::asset_fetcher::AssetType;
use crate::providers::backends::fs_cache::AssetProvider;
use crate::providers::backends::string_provider::StringProvider;
use crate::types::errors::AssetErr;
use derive_new::new;
use geo::Polygon;
use std::io::Read;
use std::sync::Arc;
use wkt::TryFromWkt;

#[async_trait]
pub trait FootprintProvider {
    async fn get_footprint(&self, bin_id: BINId) -> Result<Polygon, AssetErr>;
}

#[derive(new)]
pub struct StringBackedFootprintProvider {
    string_provider: Box<dyn StringProvider + Send + Sync>,
}

#[async_trait]
impl FootprintProvider for StringBackedFootprintProvider {
    async fn get_footprint(&self, bin_id: BINId) -> Result<Polygon, AssetErr> {
        // BINs are small <10B and inexpensive to clone (single String::clone())
        let wkt_string = self
            .string_provider
            .get_string(AssetType::BuildingFootprintWKT, bin_id.as_str())
            .await?;

        Polygon::try_from_wkt_str(&wkt_string).map_err(|err| AssetErr::AssetContentError(format!(
                "Invalid WKT string found in database for {bin_id:?}: {err}"
            )))
    }
}

#[derive(new)]
pub struct CachingFootprintProvider {
    asset_provider: Arc<dyn AssetProvider + Send + Sync>,
}

#[async_trait]
impl FootprintProvider for CachingFootprintProvider {
    async fn get_footprint(&self, bin_id: BINId) -> Result<Polygon, AssetErr> {
        let asset_id = format!("{}.wkt", bin_id.as_str());
        let mut file = self
            .asset_provider
            .get_asset(AssetType::BuildingFootprintWKT, &asset_id)
            .await?;

        let mut wkt_string = String::new();
        file.read_to_string(&mut wkt_string).map_err(|err| {
            AssetErr::LocalFileSystemError(format!(
                "Error reading WKT file for {bin_id:?}: {err}"
            ))
        })?;

        Polygon::try_from_wkt_str(&wkt_string).map_err(|err| {
            AssetErr::AssetContentError(format!(
                "Invalid WKT string in file for {bin_id:?}: {err}"
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct Captured {
        asset_type: Option<AssetType>,
        identifier: Option<String>,
    }

    struct MockStringProvider {
        result: Result<String, AssetErr>,
        captured: Arc<Mutex<Captured>>,
    }

    impl MockStringProvider {
        fn ok(wkt: &str) -> (Self, Arc<Mutex<Captured>>) {
            let captured = Arc::new(Mutex::new(Captured {
                asset_type: None,
                identifier: None,
            }));
            let mock = MockStringProvider {
                result: Ok(wkt.to_string()),
                captured: captured.clone(),
            };
            (mock, captured)
        }

        fn err(e: AssetErr) -> Self {
            MockStringProvider {
                result: Err(e),
                captured: Arc::new(Mutex::new(Captured {
                    asset_type: None,
                    identifier: None,
                })),
            }
        }
    }

    #[async_trait]
    impl StringProvider for MockStringProvider {
        async fn get_string(
            &self,
            asset_type: AssetType,
            identifier: &str,
        ) -> Result<String, AssetErr> {
            let mut c = self.captured.lock().unwrap();
            c.asset_type = Some(asset_type);
            c.identifier = Some(identifier.to_string());
            drop(c);
            match &self.result {
                Ok(s) => Ok(s.clone()),
                Err(AssetErr::AssetDownloadError(msg)) => {
                    Err(AssetErr::AssetDownloadError(msg.clone()))
                }
                Err(AssetErr::AssetNotFound(msg)) => Err(AssetErr::AssetNotFound(msg.clone())),
                Err(AssetErr::AssetContentError(msg)) => {
                    Err(AssetErr::AssetContentError(msg.clone()))
                }
                Err(AssetErr::LocalFileSystemError(msg)) => {
                    Err(AssetErr::LocalFileSystemError(msg.clone()))
                }
                Err(AssetErr::UnsupportedAssetType(msg)) => {
                    Err(AssetErr::UnsupportedAssetType(msg.clone()))
                }
            }
        }
    }

    fn make_provider(mock: MockStringProvider) -> StringBackedFootprintProvider {
        StringBackedFootprintProvider::new(Box::new(mock))
    }

    fn valid_bin() -> BINId {
        BINId::parse("1000001").unwrap()
    }

    const SIMPLE_POLYGON_WKT: &str = "POLYGON((0 0, 1 0, 1 1, 0 1, 0 0))";

    #[tokio::test]
    async fn returns_polygon_for_valid_wkt() {
        let (mock, _) = MockStringProvider::ok(SIMPLE_POLYGON_WKT);
        let result = make_provider(mock).get_footprint(valid_bin()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn passes_correct_asset_type_and_bin_id() {
        let (mock, captured) = MockStringProvider::ok(SIMPLE_POLYGON_WKT);
        make_provider(mock)
            .get_footprint(valid_bin())
            .await
            .unwrap();
        let c = captured.lock().unwrap();
        assert_eq!(c.asset_type, Some(AssetType::BuildingFootprintWKT));
        assert_eq!(c.identifier, Some("1000001".to_string()));
    }

    #[tokio::test]
    async fn returns_content_error_for_invalid_wkt() {
        let (mock, _) = MockStringProvider::ok("not valid wkt at all");
        let result = make_provider(mock).get_footprint(valid_bin()).await;
        assert!(matches!(result, Err(AssetErr::AssetContentError(_))));
    }

    #[tokio::test]
    async fn returns_content_error_for_non_polygon_wkt() {
        let (mock, _) = MockStringProvider::ok("POINT(0 0)");
        let result = make_provider(mock).get_footprint(valid_bin()).await;
        assert!(matches!(result, Err(AssetErr::AssetContentError(_))));
    }

    #[tokio::test]
    async fn propagates_download_error_from_string_provider() {
        let provider = make_provider(MockStringProvider::err(AssetErr::AssetDownloadError(
            "db error".to_string(),
        )));
        let result = provider.get_footprint(valid_bin()).await;
        assert!(matches!(result, Err(AssetErr::AssetDownloadError(_))));
    }

    #[tokio::test]
    async fn propagates_not_found_error_from_string_provider() {
        let provider = make_provider(MockStringProvider::err(AssetErr::AssetNotFound(
            "not found".to_string(),
        )));
        let result = provider.get_footprint(valid_bin()).await;
        assert!(matches!(result, Err(AssetErr::AssetNotFound(_))));
    }

    // --- CachingFootprintProvider ---

    use super::CachingFootprintProvider;
    use std::path::PathBuf;
    use std::fs::File;
    use test_temp_dir::test_temp_dir;

    struct MockAssetProvider {
        result: Result<PathBuf, AssetErr>,
        captured_id: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    }

    impl MockAssetProvider {
        fn returning_wkt(wkt: &str) -> (Self, test_temp_dir::TestTempDir) {
            let dir = test_temp_dir!();
            let path = dir.as_path_untracked().join("footprint.wkt");
            std::fs::write(&path, wkt).unwrap();
            let provider = Self {
                result: Ok(path),
                captured_id: Default::default(),
            };
            (provider, dir)
        }

        fn returning_err(err: AssetErr) -> Self {
            Self {
                result: Err(err),
                captured_id: Default::default(),
            }
        }

        fn capture_id(&self) -> std::sync::Arc<std::sync::Mutex<Option<String>>> {
            self.captured_id.clone()
        }
    }

    #[async_trait]
    impl AssetProvider for MockAssetProvider {
        fn get_local_asset_path(&self, _: AssetType, _: &str) -> PathBuf {
            PathBuf::new()
        }

        async fn get_asset(&self, _: AssetType, asset_id: &str) -> Result<File, AssetErr> {
            *self.captured_id.lock().unwrap() = Some(asset_id.to_string());
            match &self.result {
                Ok(path) => File::open(path)
                    .map_err(|e| AssetErr::LocalFileSystemError(e.to_string())),
                Err(AssetErr::AssetNotFound(msg)) => Err(AssetErr::AssetNotFound(msg.clone())),
                Err(AssetErr::AssetDownloadError(msg)) => {
                    Err(AssetErr::AssetDownloadError(msg.clone()))
                }
                Err(e) => Err(AssetErr::LocalFileSystemError(format!("{e:?}"))),
            }
        }

        async fn list_assets_of_type(&self, _: AssetType) -> Result<Vec<String>, AssetErr> {
            panic!("MockAssetProvider::list_assets_of_type not expected")
        }
    }

    fn make_caching_provider(mock: MockAssetProvider) -> CachingFootprintProvider {
        CachingFootprintProvider::new(Arc::new(mock))
    }

    #[tokio::test]
    async fn caching_returns_polygon_for_valid_wkt_file() {
        let (mock, _dir) = MockAssetProvider::returning_wkt(SIMPLE_POLYGON_WKT);
        let result = make_caching_provider(mock).get_footprint(valid_bin()).await;
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    #[tokio::test]
    async fn caching_requests_bin_dot_wkt_asset_id() {
        let (mock, _dir) = MockAssetProvider::returning_wkt(SIMPLE_POLYGON_WKT);
        let captured = mock.capture_id();
        make_caching_provider(mock)
            .get_footprint(valid_bin())
            .await
            .unwrap();
        assert_eq!(
            captured.lock().unwrap().as_deref(),
            Some("1000001.wkt")
        );
    }

    #[tokio::test]
    async fn caching_returns_content_error_for_invalid_wkt_file() {
        let (mock, _dir) = MockAssetProvider::returning_wkt("not valid wkt");
        let result = make_caching_provider(mock).get_footprint(valid_bin()).await;
        assert!(matches!(result, Err(AssetErr::AssetContentError(_))));
    }

    #[tokio::test]
    async fn caching_returns_content_error_for_non_polygon_wkt() {
        let (mock, _dir) = MockAssetProvider::returning_wkt("POINT(0 0)");
        let result = make_caching_provider(mock).get_footprint(valid_bin()).await;
        assert!(matches!(result, Err(AssetErr::AssetContentError(_))));
    }

    #[tokio::test]
    async fn caching_propagates_not_found_error() {
        let mock = MockAssetProvider::returning_err(AssetErr::AssetNotFound("missing".into()));
        let result = make_caching_provider(mock).get_footprint(valid_bin()).await;
        assert!(matches!(result, Err(AssetErr::AssetNotFound(_))));
    }

    #[tokio::test]
    async fn caching_propagates_download_error() {
        let mock =
            MockAssetProvider::returning_err(AssetErr::AssetDownloadError("network".into()));
        let result = make_caching_provider(mock).get_footprint(valid_bin()).await;
        assert!(matches!(result, Err(AssetErr::AssetDownloadError(_))));
    }
}
