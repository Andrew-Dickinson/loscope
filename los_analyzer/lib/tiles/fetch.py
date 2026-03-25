from __future__ import annotations

import os
from pathlib import Path
from typing import Protocol, runtime_checkable


@runtime_checkable
class TileBackend(Protocol):
    """Protocol for tile data source backends.

    An implementation must download both the .tif raster and .json metadata
    files for a given tile_id into a caller-supplied directory.  The files
    must be named <tile_id>.tif and <tile_id>.json on arrival.
    """

    def fetch_tile_files(self, tile_id: str, dest_dir: Path) -> None: ...


class CachingTileFetcher:
    """Wraps any TileBackend with a local disk cache.

    A tile is considered cached when both <tile_id>.tif and <tile_id>.json
    are present in cache_dir.  If either is absent the backend is asked to
    fetch both.  The cache directory is created on first use.
    """

    def __init__(self, backend: TileBackend, cache_dir: Path | str) -> None:
        self.backend = backend
        self.cache_dir = Path(cache_dir)

    def is_cached(self, tile_id: str) -> bool:
        return (
            (self.cache_dir / f"{tile_id}.tif").exists()
            and (self.cache_dir / f"{tile_id}.json").exists()
        )

    def ensure_tile(self, tile_id: str) -> None:
        """Fetch tile_id from the backend if it is not already cached."""
        if self.is_cached(tile_id):
            return
        self.cache_dir.mkdir(parents=True, exist_ok=True)
        self.backend.fetch_tile_files(tile_id, self.cache_dir)

    def ensure_tiles(self, tile_ids: list[str]) -> None:
        """Fetch all tiles in tile_ids that are not already cached."""
        for tile_id in tile_ids:
            self.ensure_tile(tile_id)


def s3_fetcher_from_env(cache_dir: Path | str) -> CachingTileFetcher:
    """Build a CachingTileFetcher backed by S3 using environment configuration.

    Required environment variables:
        LOS_S3_BUCKET  — S3 bucket name
        LOS_S3_PREFIX  — key prefix, e.g. "nyc-lidar-2021/preprocessed"

    AWS credentials are resolved by the default boto3 credential chain
    (environment variables, ~/.aws/credentials, IAM role, etc.).

    Raises KeyError if either required variable is not set.
    """
    from lib.tiles.s3_backend import S3TileBackend

    bucket = os.environ["LOS_S3_BUCKET"]
    prefix = os.environ["LOS_S3_PREFIX"]
    return CachingTileFetcher(S3TileBackend(bucket, prefix), cache_dir)
