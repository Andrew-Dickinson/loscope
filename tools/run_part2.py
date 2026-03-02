"""
Run Part 2 obstruction-detection steps 2.1 and 2.2.

Usage:
    python -m los_analyzer.run_part2 [tile_dir]

Default tile_dir: data/preprocessed
"""
import sys
from pathlib import Path

from los_analyzer.fresnel.fresnel_zone2 import compute_fresnel_zone, translate_to_nys_plane
from los_analyzer.tiles.identify import identify_tiles

GPS_A = (40.650, -73.800, 100.0)
GPS_B = (40.7173, -74.0060, 10000.0)
FREQUENCY_HZ = 5_000_000_000
ALPHA = 0.8


def run(tile_dir="data/preprocessed"):
    tile_dir = Path(tile_dir)

    print("=== Step 2.1: Compute Fresnel zone ===")
    print(f"  GPS A : {GPS_A}")
    print(f"  GPS B : {GPS_B}")
    print(f"  freq  : {FREQUENCY_HZ / 1e9:.1f} GHz   alpha={ALPHA}")

    nys_a, nys_b = translate_to_nys_plane([GPS_A, GPS_B])
    print(f"  NYS A : ({nys_a[0]:.1f}, {nys_a[1]:.1f}, {nys_a[2]:.1f})")
    print(f"  NYS B : ({nys_b[0]:.1f}, {nys_b[1]:.1f}, {nys_b[2]:.1f})")

    zone = compute_fresnel_zone(nys_a, nys_b, FREQUENCY_HZ, ALPHA)
    h = zone.widths.shape[0]
    max_w = int(zone.widths.max())
    print(f"  Output: H={h} rows, max_width={max_w} cols")
    print(f"  x_base_offset={zone.x_base_offset}  y_base_offset={zone.y_base_offset}")

    print()
    print("=== Step 2.2: Identify tiles ===")
    print(f"  tile_dir: {tile_dir}")
    tiles = identify_tiles(zone, tile_dir, require_exists=False)
    print(f"  {len(tiles)} tile(s) found:")
    for t in tiles:
        print(f"    {t}")


if __name__ == "__main__":
    td = sys.argv[1] if len(sys.argv) > 1 else "data/preprocessed"
    run(td)
