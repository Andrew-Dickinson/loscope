use crate::types::errors::AssetErr;
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::operation::get_object::GetObjectError;
use derive_new::new;
use std::collections::HashMap;
use std::path::Path;
use aws_sdk_s3::primitives::ByteStream;
use futures_util::{Stream, StreamExt};
use reqwest::Url;
use std::time::Duration;
use strum_macros::{AsRefStr, Display};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use typed_path::{Utf8UnixPath, Utf8UnixPathBuf};

const MANIFEST_FILE_NAME: &str = "_manifest.txt";
pub const ASSET_FETCH_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Hash, Eq, PartialEq, AsRefStr, Display, Copy, Clone)]
pub enum AssetType {
    OrthoImage,
    ElevationTile,
    TerrainClassificationTile,
    ObstructionIndex,
    Obstruction,
    BuildingFootprintWKT,
}

#[async_trait]
pub trait AssetFetcher {
    /// Downloads the asset from the specified remote_path of the specified asset type to the
    /// specified local path, if local_path is not successfully populated, returns Err(AssetErr)
    async fn fetch_asset(
        &self,
        asset_type: AssetType,
        remote_path: &Utf8UnixPath,
        local_path: &Path,
    ) -> Result<(), AssetErr>;
    async fn list_assets(&self, asset_type: AssetType) -> Result<Vec<String>, AssetErr>;
}

fn parse_manifest(manifest_contents: Vec<u8>) -> Result<Vec<String>, AssetErr> {
    let s = std::str::from_utf8(manifest_contents.as_ref())
        .map_err(|err| AssetErr::AssetDownloadError(
            format!("Failed to parse manifest file: {err:?}")
        ))?;

    Ok(
        s.lines()
        .filter(|&file_name| !file_name.eq(MANIFEST_FILE_NAME))
        .map(String::from)
        .collect()
    )
}


#[derive(new)]
pub struct S3AssetFetcher {
    client: aws_sdk_s3::Client,
    bucket_name: String,
    asset_type_prefixes: HashMap<AssetType, Utf8UnixPathBuf>,
}

impl S3AssetFetcher {
    async fn get_object(&self, key: Utf8UnixPathBuf) -> Result<ByteStream, AssetErr> {
        let bucket = self.bucket_name.as_str();
        let mut result = self
            .client
            .get_object()
            .bucket(bucket)
            .key(key.as_str())
            .send()
            .await
            .map_err(|err| match err {
                SdkError::ServiceError(err_details)
                if matches!(err_details.err(), GetObjectError::NoSuchKey(_)) =>
                    {
                        AssetErr::AssetNotFound(format!(
                            "{key} key not found in bucket {bucket}, {err_details:?}"
                        ))
                    }
                err_details => AssetErr::AssetDownloadError(format!(
                    "Unable to download {key} from {bucket}: {err_details:?}"
                )),
            })?;

        Ok(result.body)
    }
}

#[async_trait]
impl AssetFetcher for S3AssetFetcher {
    async fn fetch_asset(
        &self,
        asset_type: AssetType,
        remote_path: &Utf8UnixPath,
        local_path: &Path,
    ) -> Result<(), AssetErr> {
        let local_path_parent =
            local_path
                .parent()
                .ok_or(AssetErr::LocalFileSystemError(format!(
                    "Expected {local_path:?} to have valid parent"
                )))?;

        fs::create_dir_all(local_path_parent).await.map_err(|err| {
            AssetErr::LocalFileSystemError(format!(
                "Error creating directories for path {local_path_parent:?}: {err:?}"
            ))
        })?;

        let prefix =
            self.asset_type_prefixes
                .get(&asset_type)
                .ok_or(AssetErr::UnsupportedAssetType(format!(
                    "Unable to find {asset_type:?} prefix"
                )))?;

        let mut temp_file = fs::File::create(local_path).await.map_err(|err| {
            AssetErr::LocalFileSystemError(format!("Error creating file {local_path:?}: {err:?}"))
        })?;

        let key = prefix.clone().join(remote_path);

        let mut stream = self.get_object(key).await?;
        while let Some(bytes) = stream.try_next().await.map_err(|err| {
            AssetErr::AssetDownloadError(format!("Failed to read from S3 download stream: {err:?}"))
        })? {
            temp_file.write_all(&bytes).await.map_err(|err| {
                AssetErr::LocalFileSystemError(format!(
                    "Failed to write from S3 download stream to local file: {err:?}"
                ))
            })?;
        }

        temp_file.flush().await.map_err(|err| {
            AssetErr::LocalFileSystemError(format!("Failed to flush file {local_path:?}: {err:?}"))
        })?;

        Ok(())
    }

    async fn list_assets(&self, asset_type: AssetType) -> Result<Vec<String>, AssetErr> {
        let prefix =
            self.asset_type_prefixes
                .get(&asset_type)
                .ok_or(AssetErr::UnsupportedAssetType(format!(
                    "Unable to find {asset_type:?} prefix"
                )))?;

        let key = prefix.clone()
            .join(Utf8UnixPathBuf::from(MANIFEST_FILE_NAME));

        let mut stream = self.get_object(key).await?;

        let mut manifest_contents: Vec<u8> = Vec::new();
        while let Some(bytes) = stream.try_next().await.map_err(|err| {
            AssetErr::AssetDownloadError(format!("Failed to read from S3 download stream: {err:?}"))
        })? {
            manifest_contents.extend_from_slice(bytes.as_ref());
        }

        parse_manifest(manifest_contents)
    }
}

pub struct HttpAssetFetcher {
    client: reqwest::Client,
    base_url: Url,
    asset_type_prefixes: HashMap<AssetType, Utf8UnixPathBuf>,
}

impl HttpAssetFetcher {
    pub fn new(client: Option<reqwest::Client>, base_url: Url, asset_type_prefixes: HashMap<AssetType, Utf8UnixPathBuf>) -> Self {
        HttpAssetFetcher {
            client: client.unwrap_or_else(|| {
                reqwest::Client::builder()
                    .timeout(ASSET_FETCH_TIMEOUT)
                    .build()
                    .expect("Failed to build reqwest client")
            }),
            base_url,
            asset_type_prefixes,
        }
    }

    async fn get_asset(&self, asset_path: Utf8UnixPathBuf) -> Result<impl Stream<Item = reqwest::Result<bytes::Bytes>>, AssetErr> {
        let full_url = self.base_url.join(asset_path.as_str()).unwrap();
        let res = self.client
            .get(full_url.clone())
            .send()
            .await
            .map_err(|err| AssetErr::AssetDownloadError(format!(
                "Request to {full_url} failed: {err:?}"
            )))?;

        if res.status() == 404 {
            return Err(AssetErr::AssetNotFound(format!(
                "{asset_path} not found at {full_url}"
            )));
        }
        if !res.status().is_success() {
            return Err(AssetErr::AssetDownloadError(format!(
                "Request to {full_url} failed with status {}", res.status()
            )));
        }

        Ok(res.bytes_stream())
    }
}


#[async_trait]
impl AssetFetcher for HttpAssetFetcher {
    async fn fetch_asset(
        &self,
        asset_type: AssetType,
        remote_path: &Utf8UnixPath,
        local_path: &Path,
    ) -> Result<(), AssetErr> {
        let local_path_parent =
            local_path
                .parent()
                .ok_or(AssetErr::LocalFileSystemError(format!(
                    "Expected {local_path:?} to have valid parent"
                )))?;

        fs::create_dir_all(local_path_parent).await.map_err(|err| {
            AssetErr::LocalFileSystemError(format!(
                "Error creating directories for path {local_path_parent:?}: {err:?}"
            ))
        })?;

        let prefix =
            self.asset_type_prefixes
                .get(&asset_type)
                .ok_or(AssetErr::UnsupportedAssetType(format!(
                    "Unable to find {asset_type:?} prefix"
                )))?;

        let mut temp_file = fs::File::create(local_path).await.map_err(|err| {
            AssetErr::LocalFileSystemError(format!("Error creating file {local_path:?}: {err:?}"))
        })?;

        let asset_path = prefix.clone().join(remote_path);

        let mut stream = self.get_asset(asset_path).await?;
        while let Some(bytes) = stream.next().await {
            let bytes = bytes.map_err(|err| {
                AssetErr::AssetDownloadError(format!("Failed to read from HTTP download stream: {err:?}"))
            })?;

            temp_file.write_all(&bytes).await.map_err(|err| {
                AssetErr::LocalFileSystemError(format!(
                    "Failed to write from HTTP download stream to local file: {err:?}"
                ))
            })?;
        }

        temp_file.flush().await.map_err(|err| {
            AssetErr::LocalFileSystemError(format!("Failed to flush file {local_path:?}: {err:?}"))
        })?;

        Ok(())
    }

    async fn list_assets(&self, asset_type: AssetType) -> Result<Vec<String>, AssetErr> {
        let prefix =
            self.asset_type_prefixes
                .get(&asset_type)
                .ok_or(AssetErr::UnsupportedAssetType(format!(
                    "Unable to find {asset_type:?} prefix"
                )))?;

        let mut manifest_stream = self.get_asset(prefix.join(MANIFEST_FILE_NAME)).await?;
        let mut manifest_contents: Vec<u8> = Vec::new();
        while let Some(bytes) = manifest_stream.next().await {
            let bytes = bytes.map_err(|err| {
                AssetErr::AssetDownloadError(format!("Failed to read from HTTP download stream: {err:?}"))
            })?;
            manifest_contents.extend_from_slice(bytes.as_ref());
        }

        parse_manifest(manifest_contents)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use super::*;
    use crate::types::errors::AssetErr;
    use aws_sdk_s3::operation::get_object::{GetObjectError, GetObjectOutput};
    use aws_sdk_s3::primitives::ByteStream;
    use aws_sdk_s3::types::error::NoSuchKey;
    use aws_smithy_mocks::{MockResponseInterceptor, Rule, RuleMode, mock};
    use test_temp_dir::{TestTempDir, test_temp_dir};
    use typed_path::Utf8UnixPath;
    use wiremock::{MockServer, Mock, ResponseTemplate};
    use wiremock::matchers::{method, path};

    fn setup(rule: Rule) -> (S3AssetFetcher, TestTempDir) {
        let interceptor = MockResponseInterceptor::new()
            .rule_mode(RuleMode::Sequential)
            .with_rule(&rule);

        let config = aws_sdk_s3::Config::builder()
            .with_test_defaults()
            .interceptor(interceptor)
            .region(aws_config::Region::new("us-east-1"))
            .build();

        (
            S3AssetFetcher {
                client: aws_sdk_s3::Client::from_conf(config),
                bucket_name: String::from("test-bucket"),
                asset_type_prefixes: HashMap::from([(
                    AssetType::OrthoImage,
                    Utf8UnixPathBuf::from("test-prefix/ortho-img"),
                )]),
            },
            test_temp_dir!(),
        )
    }

    #[tokio::test]
    async fn test_good_download() {
        let (fetcher, temp_dir) = setup(mock!(aws_sdk_s3::Client::get_object).then_output(|| {
            GetObjectOutput::builder()
                .body(ByteStream::from_static("mock-image-bytes".as_bytes()))
                .build()
        }));

        let temp_ortho_image_path =
            temp_dir.used_by(|path| path.join("ortho_image.jpg").to_path_buf());

        let result = fetcher
            .fetch_asset(
                AssetType::OrthoImage,
                Utf8UnixPath::new("photo.jpg"),
                &temp_ortho_image_path,
            )
            .await;

        assert!(matches!(result, Ok(())));

        let file_contents = std::fs::read_to_string(&*temp_ortho_image_path).unwrap();
        assert_eq!(file_contents, "mock-image-bytes");
    }

    #[tokio::test]
    async fn test_asset_not_found() {
        let (fetcher, temp_dir) = setup(
            mock!(aws_sdk_s3::Client::get_object)
                .then_error(|| GetObjectError::NoSuchKey(NoSuchKey::builder().build())),
        );

        let temp_ortho_image_path =
            temp_dir.used_by(|path| path.join("ortho_image.jpg").to_path_buf());

        let result = fetcher
            .fetch_asset(
                AssetType::OrthoImage,
                Utf8UnixPath::new("photo.jpg"),
                &temp_ortho_image_path,
            )
            .await;

        assert!(matches!(result, Err(AssetErr::AssetNotFound(_))));
    }

    #[tokio::test]
    async fn test_unexpected_error() {
        let (fetcher, temp_dir) = setup(
            mock!(aws_sdk_s3::Client::get_object)
                .then_error(|| GetObjectError::unhandled("simulated unexpected error")),
        );

        let temp_ortho_image_path =
            temp_dir.used_by(|path| path.join("ortho_image.jpg").to_path_buf());

        let result = fetcher
            .fetch_asset(
                AssetType::OrthoImage,
                Utf8UnixPath::new("photo.jpg"),
                &temp_ortho_image_path,
            )
            .await;

        assert!(matches!(result, Err(AssetErr::AssetDownloadError(_))));
    }

    #[tokio::test]
    async fn test_local_file_error() {
        let (fetcher, _) = setup(mock!(aws_sdk_s3::Client::get_object).then_output(|| {
            GetObjectOutput::builder()
                .body(ByteStream::from_static("mock-image-bytes".as_bytes()))
                .build()
        }));

        let result = fetcher
            .fetch_asset(
                AssetType::OrthoImage,
                Utf8UnixPath::new("photo.jpg"),
                &PathBuf::from("/nonexistent-not-a-real-directory-dont-create-me/photo.jpg"),
            )
            .await;

        assert!(matches!(result, Err(AssetErr::LocalFileSystemError(_))));
    }

    fn make_list_fetcher(rule: Rule) -> S3AssetFetcher {
        let interceptor = MockResponseInterceptor::new()
            .rule_mode(RuleMode::Sequential)
            .with_rule(&rule);

        let config = aws_sdk_s3::Config::builder()
            .with_test_defaults()
            .interceptor(interceptor)
            .region(aws_config::Region::new("us-east-1"))
            .build();

        S3AssetFetcher {
            client: aws_sdk_s3::Client::from_conf(config),
            bucket_name: String::from("test-bucket"),
            asset_type_prefixes: HashMap::from([(
                AssetType::OrthoImage,
                Utf8UnixPathBuf::from("prefix/ortho"),
            )]),
        }
    }

    fn manifest_output(contents: &'static str) -> Rule {
        mock!(aws_sdk_s3::Client::get_object).then_output(move || {
            GetObjectOutput::builder()
                .body(ByteStream::from_static(contents.as_bytes()))
                .build()
        })
    }

    // --- list_assets ---

    #[tokio::test]
    async fn test_list_assets_parses_manifest() {
        let fetcher = make_list_fetcher(manifest_output("a.tif\nsub/b.tif\n"));

        let keys = fetcher.list_assets(AssetType::OrthoImage).await.unwrap();
        assert_eq!(keys, vec!["a.tif", "sub/b.tif"]);
    }

    #[tokio::test]
    async fn test_list_assets_filters_manifest_file() {
        // The manifest file itself should never be listed as an asset.
        let fetcher = make_list_fetcher(manifest_output("a.tif\n_manifest.txt\nb.tif\n"));

        let keys = fetcher.list_assets(AssetType::OrthoImage).await.unwrap();
        assert_eq!(keys, vec!["a.tif", "b.tif"]);
    }

    #[tokio::test]
    async fn test_list_assets_empty() {
        let fetcher = make_list_fetcher(manifest_output(""));

        let keys = fetcher.list_assets(AssetType::OrthoImage).await.unwrap();
        assert!(keys.is_empty());
    }

    #[tokio::test]
    async fn test_list_assets_manifest_not_found() {
        let fetcher = make_list_fetcher(
            mock!(aws_sdk_s3::Client::get_object)
                .then_error(|| GetObjectError::NoSuchKey(NoSuchKey::builder().build())),
        );

        let result = fetcher.list_assets(AssetType::OrthoImage).await;
        assert!(matches!(result, Err(AssetErr::AssetNotFound(_))));
    }

    #[tokio::test]
    async fn test_list_assets_invalid_utf8() {
        let fetcher = make_list_fetcher(mock!(aws_sdk_s3::Client::get_object).then_output(|| {
            GetObjectOutput::builder()
                .body(ByteStream::from_static(&[0xff, 0xfe]))
                .build()
        }));

        let result = fetcher.list_assets(AssetType::OrthoImage).await;
        assert!(matches!(result, Err(AssetErr::AssetDownloadError(_))));
    }

    #[tokio::test]
    async fn test_list_assets_s3_error() {
        let fetcher = make_list_fetcher(
            mock!(aws_sdk_s3::Client::get_object)
                .then_error(|| GetObjectError::unhandled("simulated S3 error")),
        );

        let result = fetcher.list_assets(AssetType::OrthoImage).await;
        assert!(matches!(result, Err(AssetErr::AssetDownloadError(_))));
    }

    #[tokio::test]
    async fn test_list_assets_unsupported_asset_type() {
        // No prefix in the map — should fail before touching S3.
        let fetcher = S3AssetFetcher {
            client: aws_sdk_s3::Client::from_conf(
                aws_sdk_s3::Config::builder()
                    .with_test_defaults()
                    .region(aws_config::Region::new("us-east-1"))
                    .build(),
            ),
            bucket_name: String::from("test-bucket"),
            asset_type_prefixes: HashMap::new(),
        };

        let result = fetcher.list_assets(AssetType::OrthoImage).await;
        assert!(matches!(result, Err(AssetErr::UnsupportedAssetType(_))));
    }

    // --- HttpAssetFetcher helpers ---

    fn make_http_fetcher(base_url: &str) -> HttpAssetFetcher {
        HttpAssetFetcher::new(
            None,
            Url::parse(base_url).unwrap(),
            HashMap::from([(
                AssetType::OrthoImage,
                Utf8UnixPathBuf::from("prefix/ortho/"),
            )]),
        )
    }

    // --- HttpAssetFetcher::fetch_asset ---

    #[tokio::test]
    async fn http_fetch_asset_good_download() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/prefix/ortho/photo.jpg"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"mock-image-bytes"))
            .mount(&server)
            .await;

        let fetcher = make_http_fetcher(&server.uri());
        let temp_dir = test_temp_dir!();
        let local_path = temp_dir.used_by(|p| p.join("photo.jpg").to_path_buf());

        let result = fetcher
            .fetch_asset(AssetType::OrthoImage, Utf8UnixPath::new("photo.jpg"), &local_path)
            .await;

        assert!(matches!(result, Ok(())));
        assert_eq!(std::fs::read_to_string(&*local_path).unwrap(), "mock-image-bytes");
    }

    #[tokio::test]
    async fn http_fetch_asset_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/prefix/ortho/missing.jpg"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let fetcher = make_http_fetcher(&server.uri());
        let temp_dir = test_temp_dir!();
        let local_path = temp_dir.used_by(|p| p.join("missing.jpg").to_path_buf());

        let result = fetcher
            .fetch_asset(AssetType::OrthoImage, Utf8UnixPath::new("missing.jpg"), &local_path)
            .await;

        assert!(matches!(result, Err(AssetErr::AssetNotFound(_))));
    }

    #[tokio::test]
    async fn http_fetch_asset_local_file_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"data"))
            .mount(&server)
            .await;

        let fetcher = make_http_fetcher(&server.uri());

        let result = fetcher
            .fetch_asset(
                AssetType::OrthoImage,
                Utf8UnixPath::new("photo.jpg"),
                &PathBuf::from("/nonexistent-not-a-real-directory-dont-create-me/photo.jpg"),
            )
            .await;

        assert!(matches!(result, Err(AssetErr::LocalFileSystemError(_))));
    }

    #[tokio::test]
    async fn http_fetch_asset_unsupported_asset_type() {
        let server = MockServer::start().await;
        let fetcher = HttpAssetFetcher::new(
            None,
            Url::parse(&server.uri()).unwrap(),
            HashMap::new(),
        );
        let temp_dir = test_temp_dir!();
        let local_path = temp_dir.used_by(|p| p.join("photo.jpg").to_path_buf());

        let result = fetcher
            .fetch_asset(AssetType::OrthoImage, Utf8UnixPath::new("photo.jpg"), &local_path)
            .await;

        assert!(matches!(result, Err(AssetErr::UnsupportedAssetType(_))));
    }

    // --- HttpAssetFetcher::list_assets ---

    #[tokio::test]
    async fn http_list_assets_parses_manifest() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/prefix/ortho/_manifest.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"a.tif\nsub/b.tif\n"))
            .mount(&server)
            .await;

        let fetcher = make_http_fetcher(&server.uri());
        let keys = fetcher.list_assets(AssetType::OrthoImage).await.unwrap();
        assert_eq!(keys, vec!["a.tif", "sub/b.tif"]);
    }

    #[tokio::test]
    async fn http_list_assets_filters_manifest_file() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/prefix/ortho/_manifest.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"a.tif\n_manifest.txt\nb.tif\n"))
            .mount(&server)
            .await;

        let fetcher = make_http_fetcher(&server.uri());
        let keys = fetcher.list_assets(AssetType::OrthoImage).await.unwrap();
        assert_eq!(keys, vec!["a.tif", "b.tif"]);
    }

    #[tokio::test]
    async fn http_list_assets_empty_manifest() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/prefix/ortho/_manifest.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b""))
            .mount(&server)
            .await;

        let fetcher = make_http_fetcher(&server.uri());
        let keys = fetcher.list_assets(AssetType::OrthoImage).await.unwrap();
        assert!(keys.is_empty());
    }

    #[tokio::test]
    async fn http_list_assets_manifest_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/prefix/ortho/_manifest.txt"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let fetcher = make_http_fetcher(&server.uri());
        let result = fetcher.list_assets(AssetType::OrthoImage).await;
        assert!(matches!(result, Err(AssetErr::AssetNotFound(_))));
    }

    #[tokio::test]
    async fn http_list_assets_invalid_utf8() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/prefix/ortho/_manifest.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(&[0xff, 0xfe]))
            .mount(&server)
            .await;

        let fetcher = make_http_fetcher(&server.uri());
        let result = fetcher.list_assets(AssetType::OrthoImage).await;
        assert!(matches!(result, Err(AssetErr::AssetDownloadError(_))));
    }

    #[tokio::test]
    async fn http_list_assets_unsupported_asset_type() {
        let server = MockServer::start().await;
        let fetcher = HttpAssetFetcher::new(
            None,
            Url::parse(&server.uri()).unwrap(),
            HashMap::new(),
        );

        let result = fetcher.list_assets(AssetType::OrthoImage).await;
        assert!(matches!(result, Err(AssetErr::UnsupportedAssetType(_))));
    }
}
