"""Flask application entry point."""
from __future__ import annotations

import os
from pathlib import Path

from flask import Flask
from flask_cors import CORS

from los_analyzer.backend.cache.private_cache import DictProvider, JobLibSerializingCache
from los_analyzer.lib.providers.dob_db_dao import DOBDBDAO
from los_analyzer.lib.providers.obstruction_provider import CachingObstructionProvider, \
    ASSET_TYPE_OBSTRUCTION_RASTER, ASSET_TYPE_OBSTRUCTION_DETAIL, ASSET_TYPE_OBSTRUCTION_INDEXES
from los_analyzer.lib.providers.ortho_provider import CachingOrthoProvider, ASSET_TYPE_ORTHO_IMAGE
from los_analyzer.lib.providers.read_through_cache import AwsS3AssetProvider
from los_analyzer.lib.providers.tile_provider import CachingTileProvider, ASSET_TYPE_TERRAIN_TIFF

app_cache = JobLibSerializingCache(DictProvider())
app = Flask(__name__, root_path=os.getcwd())
CORS(app)

LOS_S3_BUCKET = os.environ["LOS_ASSET_S3_BUCKET"]

dob_db_dao = DOBDBDAO(Path(os.environ.get("LOS_DB_PATH",  "data/nyc_dob.db")))
TILE_DIR = Path(os.environ.get("LOS_TILE_DIR", "data/preprocessed"))
tile_provider = CachingTileProvider(
    AwsS3AssetProvider(LOS_S3_BUCKET, {ASSET_TYPE_TERRAIN_TIFF: os.environ["LOS_TERRAIN_TILE_S3_PREFIX"]}),
    TILE_DIR
)

LOS_OBSTRUCTION_S3_PREFIX = os.environ["LOS_OBSTRUCTION_S3_PREFIX"]
obstruction_provider = CachingObstructionProvider(
    AwsS3AssetProvider(
        LOS_S3_BUCKET,
        {
            ASSET_TYPE_OBSTRUCTION_DETAIL: LOS_OBSTRUCTION_S3_PREFIX,
            ASSET_TYPE_OBSTRUCTION_RASTER: LOS_OBSTRUCTION_S3_PREFIX,
            ASSET_TYPE_OBSTRUCTION_INDEXES: LOS_OBSTRUCTION_S3_PREFIX
        }
    ),
    Path(os.environ.get("LOS_OBS_DIR", "data/obstructions"))
)
ortho_provider = CachingOrthoProvider(
    AwsS3AssetProvider(LOS_S3_BUCKET,{ASSET_TYPE_ORTHO_IMAGE: os.environ["LOS_ORTHOS_S3_PREFIX"]}),
    Path(os.environ.get("LOS_ORTHO_DIR", "data/orthos"))
)

@app.get("/api/healthcheck")
def hello():
    return "Healthy"

from los_analyzer.backend.endpoints.analysis import * # noqa # pylint: disable=unused-import
from los_analyzer.backend.endpoints.rooftop import * # noqa # pylint: disable=unused-import
from los_analyzer.backend.endpoints.tile_view import * # noqa # pylint: disable=unused-import
from los_analyzer.backend.endpoints.coords import * # noqa # pylint: disable=unused-import

if __name__ == "__main__":
    app.run()
