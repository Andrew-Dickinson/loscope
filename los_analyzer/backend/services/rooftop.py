"""Rooftop evaluation service.

Extracts the data pipeline from tools/evaluate_rooftop.py, returning a
JSON-serializable dict instead of writing files or HTML.
"""
from __future__ import annotations

import dataclasses
from io import BytesIO
from typing import List

import numpy as np

from los_analyzer.backend.app import app_cache
from los_analyzer.lib.building.heightmap import build_building_heightmap, RooftopHeightMap
from los_analyzer.lib.io.encoding_bytes_io import EncodingBytesIO
from los_analyzer.lib.providers.dob_db_dao import DOBDBDAO
from los_analyzer.lib.providers.tile_provider import TileProvider
from los_analyzer.lib.sample_points import get_paired_sample_points, SamplePoint


@app_cache.cache_return_value(key=lambda bin_id, _, __: bin_id)
def build_building_heightmap_cached(
    bin_id: str,
    dob_db_dao: DOBDBDAO,
    tile_provider: TileProvider,
) -> RooftopHeightMap:
    return build_building_heightmap(bin_id, dob_db_dao, tile_provider)

@app_cache.cache_return_value(
    key=lambda model, sample_spacing, mast_offset: (model.bin_id, sample_spacing, mast_offset)
)
def get_paired_sample_points_cached(model: RooftopHeightMap, sample_spacing: int, mast_offset: float) -> List[SamplePoint]:
    return get_paired_sample_points(model, sample_spacing, mast_offset)

@dataclasses.dataclass
class RooftopRaster:
    sample_points: List[SamplePoint]
    rooftop_obj: BytesIO


@app_cache.cache_return_value(key=lambda _bin_id, _: _bin_id)
def build_rooftop_obj(_bin_id: str, heightmap: np.ndarray) -> BytesIO:
    """Return OBJ file content for a building heightmap. Renders vertical surfaces only between non-zero pixels in
    the input heightmap, which basically removes the walls of the building and makes the rooftop easier to interact with
    Coordinate system: X=local easting, Y=local northing, Z=elevation (ft).
    """
    z = heightmap.astype(np.float32) / 12.0
    W, H = z.shape
    z_floor = 0.0
    vi = 1

    buf = EncodingBytesIO()

    buf.write("# Building heightmap terrain\n")
    buf.write("# X = easting (local), Y = northing (local), Z = elevation (ft)\n")
    buf.write("o heightmap\n\n")

    for xi in range(W):
        for yi in range(H):
            zt = float(z[xi, yi])
            if zt == 0.0:
                continue
            x0, y0 = float(xi), float(yi)
            x1, y1 = x0 + 1.0, y0 + 1.0

            buf.write(f"v {x0} {y0} {zt:.3f}\n")
            buf.write(f"v {x1} {y0} {zt:.3f}\n")
            buf.write(f"v {x1} {y1} {zt:.3f}\n")
            buf.write(f"v {x0} {y1} {zt:.3f}\n")
            o = vi
            buf.write(f"f {o} {o+1} {o+2} {o+3}\n")
            vi += 4

            for dxi, dyi, ax, ay, bx, by in [
                ( 0, -1, x0, y0, x1, y0),
                ( 0, +1, x1, y1, x0, y1),
                (+1,  0, x1, y0, x1, y1),
                (-1,  0, x0, y1, x0, y0),
            ]:
                nxi, nyi = xi + dxi, yi + dyi
                if 0 <= nxi < W and 0 <= nyi < H:
                    nz = float(z[nxi, nyi])
                    zb = nz
                else:
                    nz = z_floor - 1.0
                    zb = z_floor

                if zt > nz and zb != 0.0:
                    buf.write(f"v {ax} {ay} {zb:.3f}\n")
                    buf.write(f"v {bx} {by} {zb:.3f}\n")
                    buf.write(f"v {bx} {by} {zt:.3f}\n")
                    buf.write(f"v {ax} {ay} {zt:.3f}\n")
                    o = vi
                    buf.write(f"f {o} {o+1} {o+2} {o+3}\n")
                    vi += 4

    buf.seek(0)
    return buf
