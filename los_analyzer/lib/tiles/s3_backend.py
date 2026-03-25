from __future__ import annotations

import json
from pathlib import Path

import numpy as np
import tifffile

from lib.preprocessing.tile_id import TILE_SIDE_USFT, file_id_to_offset


def _sw_corner_from_tile_id(tile_id: str) -> tuple[int, int]:
    """Return (x_offset, y_offset) SW corner for a tile_id of the form '{file_id}_{xi}{yi}'."""
    file_id, suffix = tile_id.rsplit("_", 1)
    xi, yi = int(suffix[0]), int(suffix[1])
    origin = file_id_to_offset(file_id)
    return origin[0] + xi * TILE_SIDE_USFT, origin[1] + yi * TILE_SIDE_USFT


def _write_empty_tile(tile_id: str, dest_dir: Path) -> None:
    """Write a zero-filled 500×500 uint16 .tif and matching .json for tile_id."""
    x_offset, y_offset = _sw_corner_from_tile_id(tile_id)
    raster = np.zeros((TILE_SIDE_USFT, TILE_SIDE_USFT), dtype=np.uint16)
    tifffile.imwrite(str(dest_dir / f"{tile_id}.tif"), raster)
    meta = {
        "tile_id": tile_id,
        "x_offset": x_offset,
        "y_offset": y_offset,
        "raster_file": f"{tile_id}.tif",
    }
    (dest_dir / f"{tile_id}.json").write_text(json.dumps(meta, indent=2))


class S3TileBackend:
    """Fetches tile files from an S3 bucket.

    AWS credentials are resolved via the default boto3 credential chain
    (environment variables, ~/.aws/credentials, IAM role, etc.) — no
    credential configuration is done here.

    boto3 is imported lazily so that the rest of the package remains
    importable without it installed.
    """

    def __init__(self, bucket: str, prefix: str) -> None:
        self.bucket = bucket
        self.prefix = prefix.rstrip("/")

    def fetch_tile_files(self, tile_id: str, dest_dir: Path) -> None:
        """Download <tile_id>.tif and <tile_id>.json from S3 into dest_dir.

        When S3 returns a 404 (tile omitted because it was all-zero or out of bounds
        during preprocessing), write an empty stub tile instead.
        """
        import boto3
        from botocore.exceptions import ClientError

        client = boto3.client("s3")
        for ext in ("tif", "json"):
            key = f"{self.prefix}/{tile_id}.{ext}"
            dest = dest_dir / f"{tile_id}.{ext}"
            try:
                client.download_file(self.bucket, key, str(dest))
            except ClientError as exc:
                if exc.response["Error"]["Code"] in ("404", "NoSuchKey"):
                    _write_empty_tile(tile_id, dest_dir)
                    return  # both files written; nothing more to fetch
                raise
