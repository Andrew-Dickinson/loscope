"""Run evaluate_point for a hardcoded pair of NYS-plane coordinates and print the result.

Uses the same provider setup as the Flask app (env vars + local cache dirs).

Required environment variables:
    LOS_TILE_DIR                 Local cache dir for tile heightmap data
    LOS_ASSET_S3_BUCKET          S3 bucket containing preprocessed assets
    LOS_TERRAIN_TILE_S3_PREFIX   Key prefix for terrain tiles
    LOS_OBSTRUCTION_S3_PREFIX    Key prefix for obstruction assets

Usage:
    python tools/eval_point.py
"""
from __future__ import annotations

import os
from pathlib import Path

from los_analyzer.backend.cache.private_cache import JobLibSerializingCache, DictProvider, \
    CacheProvider, Key, SerializingCache
from los_analyzer.lib.evaluation.rooftop import evaluate_point
from los_analyzer.lib.providers.obstruction_provider import (
    CachingObstructionProvider,
    ASSET_TYPE_OBSTRUCTION_DETAIL,
    ASSET_TYPE_OBSTRUCTION_INDEXES,
    ASSET_TYPE_OBSTRUCTION_RASTER
)
from los_analyzer.lib.providers.read_through_cache import AwsS3AssetProvider
from los_analyzer.lib.providers.tile_provider import CachingTileProvider, ASSET_TYPE_TERRAIN_TIFF

# PT_A = (1039747.7083194072, 176149.39110709145, 329.3490199977532)
# PT_B = (1039622.934253814, 230798.89482046565, 329.34441791288555)

PT_A = (1009748.3478422969, 253099.53772897943, 251.25)
PT_B = (1000565.7271487191, 241854.0, 257.6095239708276)
FREQUENCY_HZ = 24e9

def main() -> None:
    bucket = os.environ["LOS_ASSET_S3_BUCKET"]

    tile_provider = CachingTileProvider(
        AwsS3AssetProvider(bucket, {ASSET_TYPE_TERRAIN_TIFF: os.environ["LOS_TERRAIN_TILE_S3_PREFIX"]}),
        Path(os.environ.get("LOS_TILE_DIR", "data/preprocessed")),
    )

    obstruction_provider = CachingObstructionProvider(
        AwsS3AssetProvider(
            bucket,
            {
                ASSET_TYPE_OBSTRUCTION_DETAIL: os.environ["LOS_OBSTRUCTION_S3_PREFIX"],
                ASSET_TYPE_OBSTRUCTION_RASTER: os.environ["LOS_OBSTRUCTION_S3_PREFIX"],
                ASSET_TYPE_OBSTRUCTION_INDEXES: os.environ["LOS_OBSTRUCTION_S3_PREFIX"],
            },
        ),
        Path(os.environ.get("LOS_OBS_DIR", "data/obstructions"))
    )

    print(f"point_a_nys : {PT_A}")
    print(f"point_b_nys : {PT_B}")
    print(f"frequency   : {FREQUENCY_HZ / 1e9:.1f} GHz")
    print()

    def pre_warm():
        evaluate_point(PT_A, PT_B, FREQUENCY_HZ, tile_provider, obstruction_provider)

    pre_warm()

    def real_run():
        return evaluate_point(PT_A, PT_B, FREQUENCY_HZ, tile_provider, obstruction_provider)

    result = real_run()

    print(f"status               : {result.status.value}")
    print(f"max_obstruction_full : {result.max_obstruction_full:.6f}")
    print(f"max_obstruction_part : {result.max_obstruction_partial:.6f}")
    print(f"tile_ids             : {result.tile_ids}")


if __name__ == "__main__":
    print(os.getpid())
    input("Press enter to continue")
    main()
