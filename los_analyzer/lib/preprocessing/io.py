from pathlib import Path

import tifffile

from .tile_id import tile_id_to_offset
from .tile import TileData

def save_tile(tile, out_dir):
    """Write a TileData to a .tif raster file."""
    out_dir = Path(out_dir)
    tif_path = out_dir / f"{tile.tile_id}.tif"
    tifffile.imwrite(str(tif_path), tile.raster)


def load_tile(tile_id, in_dir) -> TileData:
    """Load a TileData from a .tif file in in_dir, deriving offsets from the tile ID."""

    in_dir = Path(in_dir)
    raster = tifffile.imread(str(in_dir / f"{tile_id}.tif"))
    x_offset, y_offset = tile_id_to_offset(tile_id)

    return TileData(
        tile_id=tile_id,
        x_offset=x_offset,
        y_offset=y_offset,
        raster=raster,
    )
