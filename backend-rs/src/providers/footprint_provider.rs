use crate::building::bin_id::BINId;
use crate::providers::backends::asset_fetcher::AssetType;
use crate::providers::backends::string_provider::StringProvider;
use crate::types::errors::AssetErr;
use derive_new::new;
use geo::{Polygon};
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
}
