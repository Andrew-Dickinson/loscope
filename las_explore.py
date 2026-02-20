import laspy
import numpy as np
from PIL import Image
from scipy.ndimage import median_filter
from tqdm import tqdm

HEIGHT = 2500
WIDTH = 2500

intensity_grid = np.zeros((WIDTH, HEIGHT), dtype=np.float64)

# intensity_sum = np.zeros((WIDTH, HEIGHT), dtype=np.float64)
intensity_count = np.zeros((WIDTH, HEIGHT), dtype=np.int32)

with laspy.open('data/235.las') as f:
    origin = f.header.mins[0:2]
    extents = f.header.maxs[0:2] - f.header.mins[0:2]
    total = f.header.point_count
    with tqdm(total=total, unit=' pts', desc='Processing') as pbar:
        for points in f.chunk_iterator(10000):
            xs = ((points.x - origin[0]) / extents[0] * (WIDTH - 1)).round().astype(int)
            ys = ((points.y - origin[1]) / extents[1] * (HEIGHT - 1)).round().astype(int)
            noise_mask = (points.classification != 7) & (points.classification != 18)
            mask = (xs >= 0) & (xs < WIDTH) & (ys >= 0) & (ys < HEIGHT) & noise_mask
            # np.add.at(intensity_sum, (xs[mask], ys[mask]), points.intensity[mask])
            np.add.at(intensity_count, (xs[mask], ys[mask]), 1)
            # intensity_grid[xs[mask], ys[mask]] += 1
            intensity_grid[xs[mask], ys[mask]] = np.maximum(points.z[mask], intensity_grid[xs[mask], ys[mask]])
            pbar.update(len(points))

# intensity_grid = np.where(intensity_count > 0, intensity_sum / intensity_count, 0.0)

smoothed_grid = median_filter(intensity_grid, size=3)
composite_grid = np.where(intensity_count > 0, intensity_grid, smoothed_grid)

EXPOSURE_SCALE_VAL = 500
intensity_image = ((composite_grid - composite_grid.min()) / (EXPOSURE_SCALE_VAL - composite_grid.min()) * 255).astype(np.uint8)

img = Image.fromarray(intensity_image, 'L').rotate(90)
img.save('data/grayscale_height_no_noise.png')