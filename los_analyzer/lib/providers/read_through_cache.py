import os
from io import FileIO, BytesIO
from pathlib import Path
from typing import Optional, Dict

import boto3
from botocore.exceptions import ClientError


class AssetProvider:
    def get_asset(self, asset_type: str, remote_asset_path: str, local_asset_path: Path) -> bool:
        raise NotImplementedError()

    def sync_path(self, asset_type: str, remote_asset_path: str, local_asset_path: Path) -> bool:
        raise NotImplementedError()


class AwsS3AssetProvider(AssetProvider):
    def __init__(self, s3_bucket: str, asset_type_s3_prefixes: Dict[str, str]):
        self.client = boto3.client("s3")
        self.bucket_name = s3_bucket
        self.asset_type_prefixes = asset_type_s3_prefixes

    def get_asset(self, asset_type: str, remote_asset_path: str, local_asset_path: Path) -> bool:
        prefix = self.asset_type_prefixes.get(asset_type)
        if not prefix:
            raise KeyError(f"{asset_type} not found in {self.asset_type_prefixes}, did you forget to configure it?")

        try:
            local_asset_path.parent.mkdir(parents=True, exist_ok=True)
            self.client.download_file(self.bucket_name, str(Path(prefix) / remote_asset_path), str(local_asset_path))
            return True
        except ClientError as exc:
            if exc.response["Error"]["Code"] in ("404", "NoSuchKey"):
                return False
            raise

    def sync_path(self, asset_type: str, remote_asset_path: str, local_asset_path: Path) -> bool:
        prefix = self.asset_type_prefixes.get(asset_type)
        if not prefix:
            raise KeyError(f"{asset_type} not found in {self.asset_type_prefixes}, did you forget to configure it?")

        s3_prefix = str(Path(prefix) / remote_asset_path)
        paginator = self.client.get_paginator("list_objects_v2")
        pages = paginator.paginate(Bucket=self.bucket_name, Prefix=s3_prefix)

        found_any = False
        for page in pages:
            for obj in page.get("Contents", []):
                key = obj["Key"]
                relative = key[len(s3_prefix):].lstrip("/")
                dest = local_asset_path / relative if relative else local_asset_path
                dest.parent.mkdir(parents=True, exist_ok=True)
                self.client.download_file(self.bucket_name, key, str(dest))
                found_any = True

        return found_any


class ReadThroughCache:
    def __init__(self, upstream: AssetProvider, cache_dir: Path):
        self._upstream = upstream
        self._cache_dir = cache_dir

    def get_from_fs_cache_or_upstream(self, asset_type: str, asset_id: str) -> Optional[Path]:
        cache_path = self._cache_dir / asset_id

        if cache_path.exists():
            return cache_path

        print(f"Calling upstream for {asset_type}, {asset_id}")
        asset_found = self._upstream.get_asset(asset_type, asset_id, cache_path)
        if asset_found:
            return cache_path

        return None

    def get_folder_from_fs_cache_or_upstream(self, asset_type: str, folder_path: str) -> Optional[Path]:
        cache_path = self._cache_dir / folder_path

        if cache_path.exists():
            # TODO: We may want to eventually expire and refresh cached folders?
            return cache_path

        asset_found = self._upstream.sync_path(asset_type, folder_path, cache_path)
        if asset_found:
            return cache_path

        return None