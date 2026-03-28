from io import BytesIO
from typing import Optional

from los_analyzer.lib.io.encoding_bytes_io import EncodingBytesIO
from los_analyzer.lib.obstructions.model import Obstruction
from los_analyzer.lib.preprocessing.tile_id import tile_id_to_offset, TILE_SIDE_USFT


def create_obstruction_obj(
    obstruction: Obstruction,
    tile_id: str,
) -> Optional[BytesIO]:
    """Create OBJ for a given obstruction"""

    tile_x_offset, tile_y_offset = tile_id_to_offset(tile_id)

    local_x = obstruction.x_offset - tile_x_offset
    local_y = obstruction.y_offset - tile_y_offset

    output_buffer = EncodingBytesIO()
    f = output_buffer

    f.write(f"# Obstruction volume mesh — tile {tile_id}\n")
    f.write("# 1 unit = 1 US survey foot\n")
    f.write("# X = easting (local), Y = northing (local), Z = elevation\n\n")
    f.write(f"o obstruction_{obstruction.obstruction_id.replace('-', '_')}_{tile_id.replace('-', '_')}\n\n")

    W, H = obstruction.raster.shape  # [easting_local, northing_local]
    cell_count = 0

    # Clamp iteration to cells that fall within the tile boundary.
    xi_lo = max(0, -local_x)
    xi_hi = min(W, TILE_SIDE_USFT - local_x)
    yi_lo = max(0, -local_y)
    yi_hi = min(H, TILE_SIDE_USFT - local_y)

    if xi_hi <= xi_lo or yi_hi <= yi_lo:
        return None

    vi = 1
    for xi in range(xi_lo, xi_hi):
        for yi in range(yi_lo, yi_hi):
            val = int(obstruction.raster[xi, yi])
            if val == 0:
                continue

            zt = val / 12.0
            x0 = float(local_x + xi)
            y0 = float(local_y + yi)
            x1 = x0 + 1.0
            y1 = y0 + 1.0

            # Flat top face (CCW, normal +Z).
            f.write(f"v {x0} {y0} {zt:.3f}\n")
            f.write(f"v {x1} {y0} {zt:.3f}\n")
            f.write(f"v {x1} {y1} {zt:.3f}\n")
            f.write(f"v {x0} {y1} {zt:.3f}\n")
            o = vi
            f.write(f"f {o} {o+1} {o+2} {o+3}\n")
            vi += 4

            # Side walls: only where this cell is taller than its neighbour.
            # Neighbours outside the raster OR outside the tile boundary both
            # count as nz=0 — the wall closes down to the ground plane.
            for dxi, dyi, ax, ay, bx, by in [
                ( 0, -1, x0, y0, x1, y0),  # south (-Y)
                ( 0, +1, x1, y1, x0, y1),  # north (+Y)
                (+1,  0, x1, y0, x1, y1),  # east  (+X)
                (-1,  0, x0, y1, x0, y0),  # west  (-X)
            ]:
                nxi, nyi = xi + dxi, yi + dyi
                if xi_lo <= nxi < xi_hi and yi_lo <= nyi < yi_hi:
                    nval = int(obstruction.raster[nxi, nyi])
                    nz = nval / 12.0 if nval > 0 else 0.0
                else:
                    nz = 0.0

                if zt > nz:
                    f.write(f"v {ax} {ay} {nz:.3f}\n")
                    f.write(f"v {bx} {by} {nz:.3f}\n")
                    f.write(f"v {bx} {by} {zt:.3f}\n")
                    f.write(f"v {ax} {ay} {zt:.3f}\n")
                    o = vi
                    f.write(f"f {o} {o+1} {o+2} {o+3}\n")
                    vi += 4

            cell_count += 1

    f.write("\n")

    output_buffer.seek(0)
    return output_buffer
