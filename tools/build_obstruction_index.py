"""Build a tile-to-obstruction index from an obstruction directory.

Reads every .json file in the directory and produces a single index.json
mapping each tile_id to the list of obstruction_ids that cover it.

Usage:
    python tools/build_obstruction_index.py [--obs-dir DIR] [--out FILE]
"""
from __future__ import annotations

import argparse
import json
from collections import defaultdict
from pathlib import Path


def build_index(obs_dir: Path) -> dict[str, list[str]]:
    index: dict[str, list[str]] = defaultdict(list)
    for json_path in sorted(obs_dir.glob("*.json")):
        if json_path.name == "index.json":
            continue
        try:
            data = json.loads(json_path.read_text())
            obs_id = data["obstruction_id"]
            for tile_id in data.get("tile_ids", []):
                index[tile_id].append(obs_id)
        except Exception as exc:
            print(f"WARNING: skipping {json_path.name}: {exc}")
    return dict(index)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Build a tile-to-obstruction index JSON"
    )
    parser.add_argument(
        "--obs-dir",
        default="data/obstructions",
        metavar="DIR",
        help="Directory containing obstruction tif+json pairs (default: data/obstructions)",
    )
    parser.add_argument(
        "--out",
        default=None,
        metavar="FILE",
        help="Output path (default: <obs-dir>/index.json)",
    )
    args = parser.parse_args()

    obs_dir = Path(args.obs_dir)
    out_path = Path(args.out) if args.out else obs_dir / "index.json"

    index = build_index(obs_dir)
    out_path.write_text(json.dumps(index, indent=2))
    total_obs = sum(len(v) for v in index.values())
    print(f"Indexed {total_obs} tile-obstruction mappings across {len(index)} tile(s) → {out_path}")


if __name__ == "__main__":
    main()
