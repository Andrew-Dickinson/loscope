"""CLI: parse NYC tax lot CSV and write per-block JSON files grouped by borough.

Usage:
    python tools/parse_tax_lots.py <csv_path> [out_dir]

If out_dir is omitted, defaults to data/tax-lots/json.
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent / "src"))

from tqdm import tqdm

from lib.tax_lots.parser import parse_csv, write_json_files


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)

    csv_path = Path(sys.argv[1])
    out_dir = Path(sys.argv[2]) if len(sys.argv) > 2 else Path("data/tax-lots/json")

    print(f"Parsing {csv_path} ...")
    data = parse_csv(csv_path)

    total_lots = sum(len(lots) for blocks in data.values() for lots in blocks.values())
    total_blocks = sum(len(blocks) for blocks in data.values())
    print(f"Parsed {total_lots} lots across {total_blocks} blocks in {len(data)} boroughs")

    print(f"Writing JSON files to {out_dir} ...")
    count = write_json_files(data, out_dir)
    print(f"Wrote {count} files.")


if __name__ == "__main__":
    main()
