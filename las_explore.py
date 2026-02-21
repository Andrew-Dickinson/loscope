import os

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

def fname_int_to_coordinate(fname_str):
    if len(fname_str) > 0:
        fname_int = int(fname_str)
    else:
        fname_int = 0

    if fname_int % 5 != 0:
        return fname_int * 1000 + 500
    return fname_int * 1000

def file_id_to_offset(file_id):
    x_min = fname_int_to_coordinate(file_id[:-3])
    y_min = fname_int_to_coordinate(file_id[-3:])
    if x_min < 500000:
        x_min += 1000000

    return (x_min, y_min)

file_id = "235"
with laspy.open(f'data/{file_id}.las') as f:
    origin = file_id_to_offset(file_id)
    extents = (WIDTH, HEIGHT)
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

from scipy.ndimage import zoom as ndimage_zoom

def export_voxel_obj(grid, filename):
    os.makedirs(filename, exist_ok=True)

    with open(os.path.join(filename, "model.mtl"), 'w') as f:
        f.write("""newmtl mtl1\nKd 1.000 1.000 1.000\nd 1.0\nillum 0\nmap_Kd texture.bmp\n""")

    shifted = grid - grid.min()
    h, w = shifted.shape

    z_min, z_max = shifted.min(), grid.max()
    with open(os.path.join(filename, "model.obj"), 'w') as f:
        f.write(f"# LiDAR voxel grid — {h}x{w} cells\n")
        f.write("# Blender: File > Import > Wavefront (.obj)\n\n")

        vi = 1  # OBJ vertex indices are 1-based

        f.write("mtllib model.mtl\n")
        f.write("usemtl mtl1\n")
        f.write("o testobject\n")

        # Bottom Face
        f.write(f"v {0} {0} {0}\n")
        f.write(f"v {w} {0} {0}\n")
        f.write(f"v {w} {h} {0}\n")
        f.write(f"v {0} {h} {0}\n")
        o = vi
        f.write(f"f {o} {o + 1} {o + 2} {o + 3}\n")
        vi += 4

        vti = 1
        with tqdm(total=h * w, desc='Writing voxels', unit=' cells') as pbar:
            for y in range(h):
                for x in range(w):
                    z = shifted[x, y]
                    x0, y0 = float(x), float(y)
                    x1, y1 = x0 + 1.0, y0 + 1.0
                    zt = float(z) + 1.0

                    # Top face
                    f.write(f"v {x0} {y0} {zt}\n")
                    f.write(f"v {x1} {y0} {zt}\n")
                    f.write(f"v {x1} {y1} {zt}\n")
                    f.write(f"v {x0} {y1} {zt}\n")
                    o = vi
                    # UV Coordinates
                    f.write(f"vt {x0/w} {y0/h}\n")
                    f.write(f"vt {x1/w} {y0/h}\n")
                    f.write(f"vt {x1/w} {y1/h}\n")
                    f.write(f"vt {x0/w} {y1/h}\n")
                    vto = vti
                    f.write(f"f {o}/{vto} {o+1}/{vto+1} {o+2}/{vto+2} {o+3}/{vto+3}\n")

                    vi += 4
                    vti += 4

                    # Side faces: only emit where this cell is higher than its
                    # neighbour. The bottom of each face meets the neighbour's
                    # top (zb = neighbour_z + 1), or z=0 at the grid boundary.
                    for dy, dx, ax, ay, bx, by in [
                        (-1,  0,  x0, y0, x1, y0),  # front (-y)
                        (+1,  0,  x1, y1, x0, y1),  # back  (+y)
                        ( 0, +1,  x1, y0, x1, y1),  # right (+x)
                        ( 0, -1,  x0, y1, x0, y0),  # left  (-x)
                    ]:
                        ny, nx = y + dy, x + dx
                        nz = shifted[nx, ny] if 0 <= ny < h and 0 <= nx < w else -1
                        if z > nz:
                            zb = float(nz + 1) if nz >= 0 else 0.0
                            f.write(f"v {ax} {ay} {zb}\n")
                            f.write(f"v {bx} {by} {zb}\n")
                            f.write(f"v {bx} {by} {zt}\n")
                            f.write(f"v {ax} {ay} {zt}\n")
                            o = vi
                            f.write(f"f {o} {o+1} {o+2} {o+3}\n")
                            vi += 4

                    pbar.update(1)

    print(f"Saved: {filename}  ({h}x{w} terrain cells)")

# composite_grid is indexed [x, y] (x-major). Transpose so the export loop
# sees [y, x], then flip y so north is at the top (LiDAR y=0 is south).
export_voxel_obj(composite_grid, 'data/voxel_terrain_slice')