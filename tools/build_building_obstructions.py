"""CLI: parse NYC building footprint CSV and write obstruction tif+json files.

Usage:
    python tools/build_building_obstructions.py <csv_path> [out_dir]

If out_dir is omitted, defaults to data/obstructions/building-footprints.
"""
import shutil
import sys
from pathlib import Path

# Allow running from repo root without installing the package
sys.path.insert(0, str(Path(__file__).parent.parent / "src"))

from tqdm import tqdm
import csv

from los_analyzer.obstructions.building_footprints import parse_building_row
from los_analyzer.obstructions.io import save_obstruction


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)

    csv_path = Path(sys.argv[1])
    out_dir = Path(sys.argv[2]) if len(sys.argv) > 2 else Path("data/obstructions/building-footprints")

    if out_dir.exists():
        shutil.rmtree(out_dir)
    out_dir.mkdir(parents=True)

    # Count rows for progress bar
    with csv_path.open(encoding="utf-8") as f:
        total = sum(1 for _ in f) - 1  # subtract header

    written = 0
    skipped = 0

    with csv_path.open(newline="", encoding="utf-8") as f:
        reader = csv.DictReader(f)
        for row in tqdm(reader, total=total, desc="Buildings"):
            obs = parse_building_row(row)
            if obs is None:
                skipped += 1
                continue
            save_obstruction(obs, out_dir)
            written += 1

    print(f"Done: {written} obstructions written to {out_dir}, {skipped} rows skipped")


if __name__ == "__main__":
    main()
