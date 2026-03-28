from io import BytesIO
from typing import Optional

import numpy as np

from los_analyzer.lib.fresnel.fresnel_zone2 import FresnelZone
from los_analyzer.lib.io.encoding_bytes_io import EncodingBytesIO
from los_analyzer.lib.preprocessing.tile_id import TILE_SIDE_USFT
from los_analyzer.lib.tiles.rasterize import rasterize_stairstep_grid_for_tile


def create_zone_obj(zone: FresnelZone, tile_id: str) -> Optional[BytesIO]:
    """Create a Minecraft-style Fresnel zone volume OBJ for one tile.

    Coordinate system:
        X = easting  (local usft, origin at tile SW corner)
        Y = northing (local usft, origin at tile SW corner)
        Z = elevation (usft)

    Each zone cell is rendered as a solid slab from its bottom height to its
    top height.  Flat top and bottom faces are emitted for every cell.
    Vertical step walls fill gaps between adjacent cells (same rule as the
    terrain OBJ: top-surface wall where zt > neighbour_zt, bottom-surface wall
    where zb < neighbour_zb).  Cells at the boundary of the zone (or tile)
    get a full closure wall from zb → zt on that side.
    """

    # Build dense tile-local arrays for zone cells that overlap this tile.
    top_z = rasterize_stairstep_grid_for_tile(
        tile_id, zone.widths, zone.offsets, zone.top, (zone.x_base_offset, zone.y_base_offset)
    ) / 12.0
    bottom_z = rasterize_stairstep_grid_for_tile(
        tile_id, zone.widths, zone.offsets, zone.bottom, (zone.x_base_offset, zone.y_base_offset)
    ) / 12.0
    in_zone = rasterize_stairstep_grid_for_tile(
        tile_id,
        zone.widths,
        zone.offsets,
        np.ones(shape=zone.bottom.shape, dtype=bool),
        (zone.x_base_offset, zone.y_base_offset)
    )

    if (in_zone == False).all():
        return None

    output_buffer = EncodingBytesIO()
    f = output_buffer

    vi = 1
    f.write(f"# Fresnel zone volume mesh — tile {tile_id}\n")
    f.write("# 1 unit = 1 US survey foot\n")
    f.write("# X = easting (local), Y = northing (local), Z = elevation\n\n")
    f.write(f"o zone_{tile_id.replace('-', '_')}\n\n")

    for xi in range(TILE_SIDE_USFT):
        for yi in range(TILE_SIDE_USFT):
            if not in_zone[xi, yi]:
                continue

            zt = float(top_z[xi, yi])
            zb = float(bottom_z[xi, yi])
            x0, y0 = float(xi), float(yi)
            x1, y1 = x0 + 1.0, y0 + 1.0

            # Top face (CCW, normal +Z)
            f.write(f"v {x0} {y0} {zt:.3f}\n")
            f.write(f"v {x1} {y0} {zt:.3f}\n")
            f.write(f"v {x1} {y1} {zt:.3f}\n")
            f.write(f"v {x0} {y1} {zt:.3f}\n")
            o = vi
            f.write(f"f {o} {o+1} {o+2} {o+3}\n")
            vi += 4

            # Bottom face (reversed winding, normal -Z)
            f.write(f"v {x0} {y1} {zb:.3f}\n")
            f.write(f"v {x1} {y1} {zb:.3f}\n")
            f.write(f"v {x1} {y0} {zb:.3f}\n")
            f.write(f"v {x0} {y0} {zb:.3f}\n")
            o = vi
            f.write(f"f {o} {o+1} {o+2} {o+3}\n")
            vi += 4

            # Side walls — same edge convention as tile_to_obj.py.
            # ax,ay → bx,by traces the shared edge (CCW outward winding).
            for dxi, dyi, ax, ay, bx, by in (
                ( 0, -1, x0, y0, x1, y0),  # south (-Y)
                ( 0, +1, x1, y1, x0, y1),  # north (+Y)
                (+1,  0, x1, y0, x1, y1),  # east  (+X)
                (-1,  0, x0, y1, x0, y0),  # west  (-X)
            ):
                nxi, nyi = xi + dxi, yi + dyi
                if 0 <= nxi < TILE_SIDE_USFT and 0 <= nyi < TILE_SIDE_USFT and in_zone[nxi, nyi]:
                    nzt = float(top_z[nxi, nyi])
                    nzb = float(bottom_z[nxi, nyi])
                    # Top-surface step: fill gap where this cell is higher.
                    if zt > nzt:
                        f.write(f"v {ax} {ay} {nzt:.3f}\n")
                        f.write(f"v {bx} {by} {nzt:.3f}\n")
                        f.write(f"v {bx} {by} {zt:.3f}\n")
                        f.write(f"v {ax} {ay} {zt:.3f}\n")
                        o = vi
                        f.write(f"f {o} {o+1} {o+2} {o+3}\n")
                        vi += 4
                    # Bottom-surface step: fill gap where this cell is lower.
                    if zb < nzb:
                        f.write(f"v {ax} {ay} {zb:.3f}\n")
                        f.write(f"v {bx} {by} {zb:.3f}\n")
                        f.write(f"v {bx} {by} {nzb:.3f}\n")
                        f.write(f"v {ax} {ay} {nzb:.3f}\n")
                        o = vi
                        f.write(f"f {o} {o+1} {o+2} {o+3}\n")
                        vi += 4
                else:
                    # No zone neighbour on this side — close with a full
                    # vertical wall from bottom to top.
                    f.write(f"v {ax} {ay} {zb:.3f}\n")
                    f.write(f"v {bx} {by} {zb:.3f}\n")
                    f.write(f"v {bx} {by} {zt:.3f}\n")
                    f.write(f"v {ax} {ay} {zt:.3f}\n")
                    o = vi
                    f.write(f"f {o} {o+1} {o+2} {o+3}\n")
                    vi += 4

    output_buffer.seek(0)
    return output_buffer

