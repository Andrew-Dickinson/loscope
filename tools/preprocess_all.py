"""
Preprocess all .las files in data/nys_raw/ in parallel.

Usage:
    python tools/preprocess_all.py [out_dir]

Default out_dir: data/preprocessed
"""
import contextlib
import os
import sys
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path

from tqdm import tqdm

sys.path.insert(0, str(Path(__file__).parent.parent))
from src.lib.preprocessing.preprocess import run_preprocessing


def _worker(args):
    las_file, out_dir = args
    with open(os.devnull, "w") as devnull, contextlib.redirect_stdout(devnull):
        tiles = run_preprocessing(las_file, out_dir)
    return las_file.name, len(tiles)


def main():
    raw_dir = Path("data/nys_raw")
    out_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("data/preprocessed")

    las_files = sorted(raw_dir.glob("*.las"))
    if not las_files:
        print(f"No .las files found in {raw_dir}")
        sys.exit(1)

    n_workers = os.cpu_count() or 1
    print(f"Found {len(las_files)} file(s) · {n_workers} workers · output -> {out_dir}")

    args = [(f, out_dir) for f in las_files]
    with ProcessPoolExecutor(max_workers=n_workers) as executor:
        futures = {executor.submit(_worker, a): a[0] for a in args}
        with tqdm(total=len(las_files), desc="Preprocessing", unit="file") as bar:
            for future in as_completed(futures):
                las_file = futures[future]
                try:
                    name, n_tiles = future.result()
                    tqdm.write(f"  {name}: {n_tiles} tiles")
                except Exception as e:
                    tqdm.write(f"  ERROR {las_file.name}: {e}")
                bar.update(1)


if __name__ == "__main__":
    main()
