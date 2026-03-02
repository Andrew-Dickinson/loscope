from dataclasses import dataclass, field

import numpy as np

from .tile_id import GRID_N, TILE_SIDE_USFT, make_tile_id, tile_sw_corner


@dataclass
class TileData:
    tile_id: str
    x_offset: int   # SW corner X (NYS usft)
    y_offset: int   # SW corner Y (NYS usft)
    raster: np.ndarray  # uint16 (500, 500), axes: [easting_local, northing_local]
    obstruction_ids: list = field(default_factory=list)


def split_tiles(height_grid, file_id, origin):
    """Split a 2500×2500 uint16 grid into 25 TileData objects."""
    tiles = []
    for xi in range(GRID_N):
        for yi in range(GRID_N):
            tile_id = make_tile_id(file_id, xi, yi)
            x_off, y_off = tile_sw_corner(origin, xi, yi)
            x0, x1 = xi * TILE_SIDE_USFT, (xi + 1) * TILE_SIDE_USFT
            y0, y1 = yi * TILE_SIDE_USFT, (yi + 1) * TILE_SIDE_USFT
            raster = height_grid[x0:x1, y0:y1].copy()
            tiles.append(TileData(
                tile_id=tile_id,
                x_offset=x_off,
                y_offset=y_off,
                raster=raster,
                obstruction_ids=[],
            ))
    return tiles
