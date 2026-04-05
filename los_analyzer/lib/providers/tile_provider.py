from pathlib import Path
from typing import Optional

import numpy as np
from cachetools.func import lru_cache
from tifffile import tifffile

from los_analyzer.lib.preprocessing.tile import TileData
from los_analyzer.lib.preprocessing.tile_id import tile_id_to_offset, TILE_SIDE_USFT
from los_analyzer.lib.providers.read_through_cache import ReadThroughCache, AssetProvider

class TileProvider:
    def get_tile(self, tile_id: str) -> Optional[TileData]:
        raise NotImplementedError()

    def get_tile_tiff_path(self, tile_id: str) -> Optional[Path]:
        raise NotImplementedError()

ASSET_TYPE_TERRAIN_TIFF = "TERRAIN_TIFF"

class CachingTileProvider(ReadThroughCache, TileProvider):
    def __init__(self, upstream: AssetProvider, tile_dir: Path):
        super().__init__(upstream, tile_dir)

    @lru_cache(maxsize=1024)
    def get_tile(self, tile_id: str) -> Optional[TileData]:
        tiff_file_name = f"{tile_id}.tif"
        tile_path = self.get_from_fs_cache_or_upstream(ASSET_TYPE_TERRAIN_TIFF, tiff_file_name)
        if not tile_path:
            # As an optimization, we chose not to store tiles which are all zeros, but analyses may request these
             # TODO: Warn the user for the case where their analysis is trying to query a tile outside the city
             #     (might require an extra lookup here to see if the tile ID falls outside some kind of index)
             _write_empty_tile(tile_path)
             return tile_path

        raster = tifffile.imread(tile_path)
        x_offset, y_offset = tile_id_to_offset(tile_id)

        return TileData(
            tile_id=tile_id,
            x_offset=x_offset,
            y_offset=y_offset,
            raster=raster,
        )

    def get_tile_tiff_path(self, tile_id: str) -> Path:
        return self.get_from_fs_cache_or_upstream(ASSET_TYPE_TERRAIN_TIFF, f"{tile_id}.tif")


def _write_empty_tile(tile_path: Path) -> None:
    """Write a zero-filled 500×500 uint16 .tif."""
    raster = np.zeros((TILE_SIDE_USFT, TILE_SIDE_USFT), dtype=np.uint16)
    tifffile.imwrite(str(tile_path), raster)