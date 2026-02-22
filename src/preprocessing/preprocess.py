"""
Preprocess a single NYS LiDAR .las file into 25 rasterized tiles.

Usage:
    python -m src.preprocessing.preprocess <las_file> [out_dir]

Default out_dir: data/preprocessed
"""
import sys
from pathlib import Path

from .io import save_tile
from .rasterize import build_height_grid, fill_gaps
from .tile import split_tiles
from .tile_id import file_id_to_offset


def run_preprocessing(las_file, out_dir="data/preprocessed"):
    las_file = Path(las_file)
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    file_id = las_file.stem
    origin = file_id_to_offset(file_id)

    print(f"Processing {las_file.name}  origin={origin}")
    height_grid, data_count = build_height_grid(str(las_file), origin)

    print("Applying gap fill...")
    filled = fill_gaps(height_grid, data_count)

    print("Splitting into tiles and saving...")
    tiles = split_tiles(filled, file_id, origin)
    for tile in tiles:
        save_tile(tile, out_dir)

    print(f"Wrote {len(tiles)} tiles to {out_dir}/")
    return tiles


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)
    las = sys.argv[1]
    out = sys.argv[2] if len(sys.argv) > 2 else "data/preprocessed"
    run_preprocessing(las, out)
