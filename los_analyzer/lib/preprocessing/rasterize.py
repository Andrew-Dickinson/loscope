import laspy
import numpy as np
from scipy.ndimage import median_filter

from .tile_id import LAS_SIDE_USFT

MAX_Z_USFT = 2000  # ~1,776 ft is NYC's tallest structure; anything above this is noise


def build_height_grid(las_file, origin):
    """Read a LAS file and produce a float64 max-height grid and a data-count grid."""
    height_grid = np.zeros((LAS_SIDE_USFT, LAS_SIDE_USFT), dtype=np.float64)
    data_count = np.zeros((LAS_SIDE_USFT, LAS_SIDE_USFT), dtype=np.int32)

    with laspy.open(las_file) as f:
        for chunk in f.chunk_iterator(10000):
            xs = np.floor(np.asarray(chunk.x) - origin[0]).astype(int)
            ys = np.floor(np.asarray(chunk.y) - origin[1]).astype(int)
            z = np.asarray(chunk.z)
            noise_mask = (np.asarray(chunk.classification) != 7) & (np.asarray(chunk.classification) != 18)
            height_mask = z < MAX_Z_USFT
            mask = (
                (xs >= 0) & (xs < LAS_SIDE_USFT) &
                (ys >= 0) & (ys < LAS_SIDE_USFT) &
                noise_mask & height_mask
            )
            np.maximum.at(height_grid, (xs[mask], ys[mask]), z[mask])
            np.add.at(data_count, (xs[mask], ys[mask]), 1)

    return height_grid, data_count


def fill_gaps(height_grid, data_count):
    """Apply median-filter gap fill to no-data pixels and convert to uint16 inches."""
    smoothed = median_filter(height_grid, size=3)
    filled = np.where(data_count > 0, height_grid, smoothed)
    return np.clip(np.round(filled * 12), 0, 65535).astype(np.uint16)
