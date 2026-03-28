"""Set required environment variables before los_analyzer.backend.app is imported."""
import os

os.environ.setdefault("LOS_ASSET_S3_BUCKET", "test-bucket")
os.environ.setdefault("LOS_TERRAIN_TILE_S3_PREFIX", "tiles")
os.environ.setdefault("LOS_OBSTRUCTION_S3_PREFIX", "obstructions")
os.environ.setdefault("LOS_ORTHOS_S3_PREFIX", "orthos")
