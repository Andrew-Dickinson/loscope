from pathlib import Path

import tifffile

from .tile_id import file_id_to_offset, TILE_SIDE_USFT


def save_tile(tile, out_dir):
    """Write a TileData to a .tif raster file."""
    out_dir = Path(out_dir)
    tif_path = out_dir / f"{tile.tile_id}.tif"
    tifffile.imwrite(str(tif_path), tile.raster)


def load_tile(tile_id, in_dir):
    """Load a TileData from a .tif file in in_dir, deriving offsets from the tile ID."""
    from .tile import TileData

    in_dir = Path(in_dir)
    raster = tifffile.imread(str(in_dir / f"{tile_id}.tif"))

    parts = tile_id.rsplit("_", 1)
    file_id = parts[0]
    xi, yi = int(parts[1][0]), int(parts[1][1])
    origin = file_id_to_offset(file_id)
    x_offset = origin[0] + xi * TILE_SIDE_USFT
    y_offset = origin[1] + yi * TILE_SIDE_USFT

    return TileData(
        tile_id=tile_id,
        x_offset=x_offset,
        y_offset=y_offset,
        raster=raster,
    )
