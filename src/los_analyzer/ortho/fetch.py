"""Lazy S3 cache for ortho JP2 imagery tiles.

Ortho files retain the original LAS tile grid: one JP2 per LAS file_id,
covering a 2500×2500 usft area at 2 pixels/usft (5000×5000 px).

S3 layout expected:
    <prefix>/<file_id_zero_padded_6>.jp2

Environment variables:
    LOS_ORTHO_S3_BUCKET  — S3 bucket
    LOS_ORTHO_S3_PREFIX  — key prefix, e.g. "ortho/2021"
"""
from __future__ import annotations

import os
from pathlib import Path


class OrthoFetcher:
    """Downloads ortho JP2 files from S3 and caches them locally.

    file_id values (e.g. "2245") are zero-padded to 6 digits when
    building S3 keys and local filenames, matching the naming convention
    used by the NYC ortho dataset (e.g. "002245.jp2").
    """

    def __init__(self, bucket: str, prefix: str, cache_dir: Path) -> None:
        self.bucket = bucket
        self.prefix = prefix.rstrip("/")
        self.cache_dir = Path(cache_dir)

    def jp2_path(self, file_id: str) -> Path:
        return self.cache_dir / f"{file_id.zfill(6)}.jp2"

    def is_cached(self, file_id: str) -> bool:
        return self.jp2_path(file_id).exists()

    def _s3_key(self, file_id: str) -> str:
        return f"{self.prefix}/{file_id.zfill(6)}.jp2"

    def ensure_for_tile_ids(self, tile_ids: list[str]) -> set[str]:
        """Ensure ortho JP2s are cached for all LAS file_ids referenced by tile_ids.

        Returns the set of file_ids for which a JP2 is available locally.
        """
        import boto3
        from botocore.exceptions import ClientError

        # Each tile_id like "2245_32" → file_id "2245"
        file_ids = {tid.rsplit("_", 1)[0] for tid in tile_ids if "_" in tid}
        self.cache_dir.mkdir(parents=True, exist_ok=True)
        client = boto3.client("s3")
        available: set[str] = set()

        for fid in sorted(file_ids):
            if self.is_cached(fid):
                available.add(fid)
                continue
            key = self._s3_key(fid)
            dest = self.jp2_path(fid)
            try:
                client.download_file(self.bucket, key, str(dest))
                available.add(fid)
                print(f"    Downloaded ortho: {dest.name}")
            except ClientError as e:
                code = e.response["Error"]["Code"]
                if code in ("404", "NoSuchKey"):
                    print(f"    Ortho not found on S3: {key}")
                else:
                    print(f"    Ortho fetch error for {fid}: {e}")

        return available


def ortho_fetcher_from_env(cache_dir: Path | str) -> OrthoFetcher | None:
    """Return an OrthoFetcher if LOS_ORTHO_S3_BUCKET / LOS_ORTHO_S3_PREFIX are set."""
    bucket = os.environ.get("LOS_ORTHO_S3_BUCKET")
    prefix = os.environ.get("LOS_ORTHO_S3_PREFIX")
    if not bucket or not prefix:
        return None
    return OrthoFetcher(bucket, prefix, Path(cache_dir))
