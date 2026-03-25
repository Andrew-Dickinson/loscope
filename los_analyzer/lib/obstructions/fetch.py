"""Lazy S3-backed cache for obstruction tif+json pairs.

S3 layout expected under a versioned prefix, e.g. ``obstructions/2026-03-08/``:

    <prefix>/<category>.json          -- tile-to-obstruction index for one category
    <prefix>/<category>/<uuid>.json   -- obstruction metadata
    <prefix>/<category>/<uuid>.tif    -- obstruction raster

The index files are discovered by listing the S3 prefix.  Index files are
cached under ``<cache_dir>/_indexes/``.  Obstruction pairs are cached flat
under ``<cache_dir>/`` as ``<uuid>.json`` / ``<uuid>.tif``.

Configure via environment variables:
    LOS_OBS_S3_BUCKET  -- S3 bucket name
    LOS_OBS_S3_PREFIX  -- key prefix, e.g. "obstructions/2026-03-08"
"""
from __future__ import annotations

import json
import os
from pathlib import Path


class ObstructionFetcher:
    """Downloads obstruction files from S3 into a local flat cache directory.

    Call :meth:`ensure_for_tiles` with the tile IDs that overlap the Fresnel
    zone; it returns the set of obstruction IDs now present in the cache.
    """

    def __init__(self, bucket: str, prefix: str, cache_dir: Path) -> None:
        self.bucket = bucket
        self.prefix = prefix.rstrip("/")
        self.cache_dir = Path(cache_dir)

    @property
    def _index_dir(self) -> Path:
        d = self.cache_dir / "_indexes"
        d.mkdir(parents=True, exist_ok=True)
        return d

    def is_obs_cached(self, obs_id: str) -> bool:
        return (
            (self.cache_dir / f"{obs_id}.tif").exists()
            and (self.cache_dir / f"{obs_id}.json").exists()
        )

    def ensure_for_tiles(self, tile_ids: list[str]) -> set[str]:
        """Fetch all obstructions relevant to tile_ids and return their IDs.

        Downloads category index files from S3 (cached locally), collects the
        obstruction IDs that overlap any of the given tiles, then downloads
        each missing tif+json pair into cache_dir.
        """
        import boto3

        client = boto3.client("s3")
        tile_set = set(tile_ids)

        categories = self._list_categories(client)
        needed: dict[str, str] = {}  # obs_id -> category name

        for cat in categories:
            index = self._load_category_index(client, cat)
            for tile_id, obs_ids in index.items():
                if tile_id in tile_set:
                    for obs_id in obs_ids:
                        needed[obs_id] = cat

        self.cache_dir.mkdir(parents=True, exist_ok=True)
        for obs_id, cat in needed.items():
            if not self.is_obs_cached(obs_id):
                for ext in ("json", "tif"):
                    key = f"{self.prefix}/{cat}/{obs_id}.{ext}"
                    dest = self.cache_dir / f"{obs_id}.{ext}"
                    client.download_file(self.bucket, key, str(dest))

        return set(needed.keys())

    def _list_categories(self, client) -> list[str]:
        """Return category names by listing *.json files at the prefix level."""
        resp = client.list_objects_v2(
            Bucket=self.bucket,
            Prefix=self.prefix + "/",
            Delimiter="/",
        )
        categories = []
        for obj in resp.get("Contents", []):
            key = obj["Key"]
            if key.endswith(".json"):
                name = key[len(self.prefix) + 1 : -len(".json")]
                if name:
                    categories.append(name)
        return categories

    def _load_category_index(self, client, category: str) -> dict[str, list[str]]:
        """Download (if needed) and return the tile→[obs_id] index for category."""
        local = self._index_dir / f"{category}.json"
        if not local.exists():
            key = f"{self.prefix}/{category}.json"
            client.download_file(self.bucket, key, str(local))
        return json.loads(local.read_text())


def obs_fetcher_from_env(cache_dir: Path) -> ObstructionFetcher | None:
    """Build an ObstructionFetcher from environment variables, or return None.

    Required environment variables:
        LOS_OBS_S3_BUCKET  -- S3 bucket name
        LOS_OBS_S3_PREFIX  -- key prefix, e.g. "obstructions/2026-03-08"
    """
    bucket = os.environ.get("LOS_OBS_S3_BUCKET")
    prefix = os.environ.get("LOS_OBS_S3_PREFIX")
    if not bucket or not prefix:
        return None
    return ObstructionFetcher(bucket, prefix, cache_dir)
