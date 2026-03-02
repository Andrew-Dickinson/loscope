from __future__ import annotations

from pathlib import Path


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
        """Download <tile_id>.tif and <tile_id>.json from S3 into dest_dir."""
        import boto3

        client = boto3.client("s3")
        for ext in ("tif", "json"):
            key = f"{self.prefix}/{tile_id}.{ext}"
            dest = dest_dir / f"{tile_id}.{ext}"
            client.download_file(self.bucket, key, str(dest))
