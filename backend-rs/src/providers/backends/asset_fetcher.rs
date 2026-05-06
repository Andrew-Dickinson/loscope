use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::operation::get_object::GetObjectError;
use derive_new::new;
use typed_path::{Utf8UnixPath, Utf8UnixPathBuf};
use crate::types::errors::AssetErr;
use strum_macros::{AsRefStr, Display};
use tokio::fs;

#[derive(Debug, Hash, Eq, PartialEq, AsRefStr, Display, Copy, Clone)]
pub enum AssetType {
    OrthoImage,
    ElevationTile,
    BuildingFootprintWKT
}

#[async_trait]
pub trait AssetFetcher {
    /// Downloads the asset from the specified remote_path of the specified asset type to the
    /// specified local path, if local_path is not successfully populated, returns Err(AssetErr)
    async fn fetch_asset(&self, asset_type: AssetType, remote_path: &Utf8UnixPath, local_path: &Path) -> Result<(), AssetErr>;
}

#[derive(new)]
pub struct S3AssetFetcher {
    client: aws_sdk_s3::Client,
    bucket_name: String,
    asset_type_prefixes: HashMap<AssetType, Utf8UnixPathBuf>,
}

#[async_trait]
impl AssetFetcher for S3AssetFetcher {
    async fn fetch_asset(&self, asset_type: AssetType, remote_path: &Utf8UnixPath, local_path: &Path) -> Result<(), AssetErr> {
        let local_path_parent = local_path.parent().ok_or(
            AssetErr::LocalFileSystemError(format!("Expected {local_path:?} to have valid parent"))
        )?;

        fs::create_dir_all(local_path_parent).await.map_err(
            |err| AssetErr::LocalFileSystemError(
                format!("Error creating directories for path {local_path_parent:?}: {err:?}")
            )
        )?;

        let prefix = self.asset_type_prefixes.get(&asset_type)
            .ok_or(AssetErr::UnsupportedAssetType(format!("Unable to find {asset_type:?} prefix")))?;

        let local_path_temp_owned = {
            let mut local_path_temp_mut = local_path.to_path_buf().into_os_string();
            local_path_temp_mut.push(".tmp");
            PathBuf::from(local_path_temp_mut)
        };
        let local_path_temp = local_path_temp_owned.as_path();

        let mut temp_file = File::create(local_path_temp)
            .map_err(|err| AssetErr::LocalFileSystemError(format!("Error opening temp file {local_path_temp:?}: {err:?}")))?;

        let bucket = self.bucket_name.as_str();
        let key = prefix.clone().join(remote_path);

        let mut result = self.client.get_object()
            .bucket(bucket)
            .key(key.as_str())
            .send()
            .await
            .map_err(|err| match err {
                SdkError::ServiceError(err_details)
                if matches!(err_details.err(), GetObjectError::NoSuchKey(_)) => {
                    AssetErr::AssetNotFound(
                        format!("{key} key not found in bucket {bucket}, {err_details:?}")
                    )
                }
                err_details => AssetErr::AssetDownloadError(
                    format!("Unable to download {key} from {bucket}: {err_details:?}")
                )
            })?;

        while let Some(bytes) = result.body.try_next().await.map_err(|err| {
            AssetErr::AssetDownloadError(format!("Failed to read from S3 download stream: {err:?}"))
        })? {
            temp_file.write_all(&bytes).map_err(|err| {
                AssetErr::LocalFileSystemError(format!(
                    "Failed to write from S3 download stream to local file: {err:?}"
                ))
            })?;
        }

        std::fs::rename(local_path_temp, local_path).or_else(|err| {
            Err(AssetErr::LocalFileSystemError(
                format!(
                    "Failed to move temp file {local_path_temp:?} \
                    to output destination {local_path:?}: {err}"
                )
            ))
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_s3::operation::get_object::{GetObjectError, GetObjectOutput};
    use aws_sdk_s3::primitives::ByteStream;
    use aws_sdk_s3::types::error::NoSuchKey;
    use aws_smithy_mocks::{mock, MockResponseInterceptor, Rule, RuleMode};
    use test_temp_dir::{test_temp_dir, TestTempDir};
    use typed_path::Utf8UnixPath;
    use crate::types::errors::AssetErr;

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
                asset_type_prefixes: HashMap::from([
                    (AssetType::OrthoImage, Utf8UnixPathBuf::from("test-prefix/ortho-img")),
                ])
            },
            test_temp_dir!()
        )
    }

    #[tokio::test]
    async fn test_good_download() {
        let (fetcher, temp_dir) = setup(
            mock!(aws_sdk_s3::Client::get_object)
                .then_output(|| GetObjectOutput::builder()
                    .body(ByteStream::from_static("mock-image-bytes".as_bytes()))
                    .build()
                )
        );

        let temp_ortho_image_path = temp_dir.used_by(|path| {
            path.join("ortho_image.jpg").to_path_buf()
        });

        let result = fetcher.fetch_asset(
            AssetType::OrthoImage,
            &Utf8UnixPath::new("photo.jpg"),
            &temp_ortho_image_path,
        ).await;

        assert!(matches!(result, Ok(())));

        let file_contents = std::fs::read_to_string(&*temp_ortho_image_path).unwrap();
        assert_eq!(file_contents, "mock-image-bytes");
    }

    #[tokio::test]
    async fn test_asset_not_found() {
        let (fetcher, temp_dir) = setup(
            mock!(aws_sdk_s3::Client::get_object)
                .then_error(|| GetObjectError::NoSuchKey(
                    NoSuchKey::builder().build()
                ))
        );

        let temp_ortho_image_path = temp_dir.used_by(|path| {
            path.join("ortho_image.jpg").to_path_buf()
        });

        let result = fetcher.fetch_asset(
            AssetType::OrthoImage,
            &Utf8UnixPath::new("photo.jpg"),
            &temp_ortho_image_path,
        ).await;

        assert!(matches!(result, Err(AssetErr::AssetNotFound(_))));
    }

    #[tokio::test]
    async fn test_unexpected_error() {
        let (fetcher, temp_dir) = setup(
            mock!(aws_sdk_s3::Client::get_object)
                .then_error(|| GetObjectError::unhandled("simulated unexpected error"))
        );

        let temp_ortho_image_path = temp_dir.used_by(|path| {
            path.join("ortho_image.jpg").to_path_buf()
        });

        let result = fetcher.fetch_asset(
            AssetType::OrthoImage,
            &Utf8UnixPath::new("photo.jpg"),
            &temp_ortho_image_path,
        ).await;

        assert!(matches!(result, Err(AssetErr::AssetDownloadError(_))));
    }

    #[tokio::test]
    async fn test_local_file_error() {
        let (fetcher, _) = setup(
            mock!(aws_sdk_s3::Client::get_object)
                .then_output(|| GetObjectOutput::builder()
                    .body(ByteStream::from_static("mock-image-bytes".as_bytes()))
                    .build()
                )
        );

        let result = fetcher.fetch_asset(
            AssetType::OrthoImage,
            &Utf8UnixPath::new("photo.jpg"),
            &PathBuf::from("/nonexistent-not-a-real-directory-dont-create-me/photo.jpg"),
        ).await;

        assert!(matches!(result, Err(AssetErr::LocalFileSystemError(_))));
    }
}
