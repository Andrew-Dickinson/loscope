import numpy as np

from los_analyzer.lib.preprocessing.tile_id import TILE_SIDE_USFT, tile_id_to_offset


def rasterize_stairstep_grid_for_tile(
    tile_id: str,
    widths: np.ndarray,
    offsets: np.ndarray,
    values: np.ndarray,
    grid_base_offset: tuple[int, int]
) -> np.ndarray:
    output_values = np.zeros((TILE_SIDE_USFT, TILE_SIDE_USFT), dtype=values.dtype)

    e0, n0 = tile_id_to_offset(tile_id)
    x_base_offset, y_base_offset = grid_base_offset

    grid_total_height = int(widths.shape[0])
    i_start = max(0, n0 - y_base_offset)
    i_end = min(grid_total_height, n0 + TILE_SIDE_USFT - y_base_offset)
    if i_start >= i_end:
        return output_values

    for i in range(i_start, i_end):
        width = int(widths[i])
        if width == 0:
            continue
        e_row_start = x_base_offset + int(offsets[i])
        overlap_e_start = max(e_row_start, e0)
        overlap_e_end = min(e_row_start + width, e0 + TILE_SIDE_USFT)
        if overlap_e_start >= overlap_e_end:
            continue
        col_start = overlap_e_start - e_row_start
        col_end = overlap_e_end - e_row_start

        vals = values[i, col_start:col_end]
        nonzero = vals > 0
        if not nonzero.any():
            continue

        out_col = overlap_e_start - e0
        out_row = i - (n0 - y_base_offset)  # local northing from tile SW; i_start is wrong when zone starts N of tile SW
        output_values[out_row, out_col:out_col + len(vals)] = vals

    return output_values
