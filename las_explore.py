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
composite_grid = np.where(intensity_count > 0, intensity_grid, smoothed_grid)[500:1000, 1000:1500]

EXPOSURE_SCALE_VAL = 500
intensity_image = ((composite_grid - composite_grid.min()) / (EXPOSURE_SCALE_VAL - composite_grid.min()) * 255).astype(np.uint8)

img = Image.fromarray(intensity_image, 'L').rotate(90)
img.save('data/grayscale_height_no_noise_slice.png')

# --- Voxel OBJ export for Blender ---
VOXEL_RESOLUTION = 500  # Downsample to this grid size (adjust for detail vs file size)
VOXEL_Z_LEVELS = 255     # Quantise heights into this many discrete levels

from scipy.ndimage import zoom as ndimage_zoom

def export_voxel_obj(grid, filename, resolution=VOXEL_RESOLUTION, z_levels=VOXEL_Z_LEVELS):
    small = ndimage_zoom(grid, (resolution / grid.shape[0], resolution / grid.shape[1]))
    h, w = small.shape

    z_min, z_max = small.min(), small.max()
    voxel_z = np.round((small - z_min) / (z_max - z_min) * (z_levels - 1)).astype(int)

    with open(filename, 'w') as f:
        f.write(f"# LiDAR voxel grid — {h}x{w} cells, {z_levels} height levels\n")
        f.write("# Blender: File > Import > Wavefront (.obj)\n\n")

        vi = 1  # OBJ vertex indices are 1-based
        with tqdm(total=h * w, desc='Writing voxels', unit=' cubes') as pbar:
            for y in range(h):
                for x in range(w):
                    z = int(voxel_z[y, x])
                    x0, y0, z0 = float(x), float(y), 0.0
                    x1, y1, z1 = x0 + 1.0, y0 + 1.0, float(z) + 1.0

                    # 8 cube corners
                    f.write(f"v {x0} {y0} {z0}\n")  # 0 bottom-front-left
                    f.write(f"v {x1} {y0} {z0}\n")  # 1
                    f.write(f"v {x1} {y1} {z0}\n")  # 2
                    f.write(f"v {x0} {y1} {z0}\n")  # 3
                    f.write(f"v {x0} {y0} {z1}\n")  # 4 top-front-left
                    f.write(f"v {x1} {y0} {z1}\n")  # 5
                    f.write(f"v {x1} {y1} {z1}\n")  # 6
                    f.write(f"v {x0} {y1} {z1}\n")  # 7

                    o = vi
                    f.write(f"f {o}   {o+3} {o+2} {o+1}\n")  # bottom  (-z)
                    f.write(f"f {o+4} {o+5} {o+6} {o+7}\n")  # top     (+z)
                    f.write(f"f {o}   {o+1} {o+5} {o+4}\n")  # front   (-y)
                    f.write(f"f {o+2} {o+6} {o+5} {o+1}\n")  # right   (+x)
                    f.write(f"f {o+3} {o+7} {o+6} {o+2}\n")  # back    (+y)
                    f.write(f"f {o}   {o+4} {o+7} {o+3}\n")  # left    (-x)

                    vi += 8
                    pbar.update(1)

    print(f"Saved: {filename}  ({h}x{w} voxels, ~{vi//8} cubes)")

# composite_grid is indexed [x, y] (x-major). Transpose so the export loop
# sees [y, x], then flip y so north is at the top (LiDAR y=0 is south).
export_voxel_obj(composite_grid, 'data/voxel_terrain_slice.obj')