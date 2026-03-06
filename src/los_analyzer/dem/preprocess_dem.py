"""Split a citywide DEM GeoTIFF into 500-usft canonical grid tiles.

The DEM is expected to be in EPSG:6539+6360 with 1 usft/pixel resolution and
heights encoded in US survey feet.  Output tiles use the same .tif + .json
format as the LiDAR preprocessor so they can be loaded by the existing
pipeline.

CLI usage:
    python -m los_analyzer.dem.preprocess_dem <dem_tif> [out_dir]
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np
import tifffile
from tqdm import tqdm

from los_analyzer.preprocessing.tile_id import TILE_SIDE_USFT
from los_analyzer.tiles.identify import _tile_id_from_sw_corner

# NW (top-left) corner of the citywide DEM, in NYS state plane (easting, northing).
# Every pixel's NW corner is at (DEM_ORIGIN_E + col, DEM_ORIGIN_N - row).
DEM_ORIGIN_E: int = 910720
DEM_ORIGIN_N: int = 275160


def dem_tile_sw_corners(dem_h: int, dem_w: int) -> list[tuple[int, int]]:
    """Return SW corners of every 500-usft canonical tile that overlaps the DEM.

    The DEM covers:
        easting  [DEM_ORIGIN_E,       DEM_ORIGIN_E + dem_w)
        northing [DEM_ORIGIN_N - dem_h, DEM_ORIGIN_N)

    Tile SW corners are snapped to the 500-usft grid and enumerated from the
    SW corner of the DEM extent outward.
    """
    n_min = DEM_ORIGIN_N - dem_h
    e_max = DEM_ORIGIN_E + dem_w - 1
    n_max = DEM_ORIGIN_N - 1

    e_start = (DEM_ORIGIN_E // TILE_SIDE_USFT) * TILE_SIDE_USFT
    n_start = (n_min // TILE_SIDE_USFT) * TILE_SIDE_USFT

    corners: list[tuple[int, int]] = []
    e = e_start
    while e <= e_max:
        n = n_start
        while n <= n_max:
            corners.append((e, n))
            n += TILE_SIDE_USFT
        e += TILE_SIDE_USFT
    return corners


def extract_dem_tile(dem: np.ndarray, e_sw: int, n_sw: int) -> np.ndarray | None:
    """Extract a 500×500 uint16 raster (inches) for a single canonical tile.

    Pixel (row, col) in the DEM represents the 1-usft square whose NW corner is
    at (DEM_ORIGIN_E + col, DEM_ORIGIN_N - row), so it covers:
        easting  [DEM_ORIGIN_E + col,     DEM_ORIGIN_E + col + 1)
        northing [DEM_ORIGIN_N - row - 1, DEM_ORIGIN_N - row)

    The returned array has axes [easting_local, northing_local] matching the
    TileData convention, where index [i, j] is the height at
    (e_sw + i, n_sw + j) in inches (uint16).  Areas outside the DEM are
    filled with 0.

    Returns None if the tile has no overlap with the DEM at all.
    """
    dem_h, dem_w = dem.shape

    col_start = e_sw - DEM_ORIGIN_E           # westernmost col (inclusive)
    col_end   = col_start + TILE_SIDE_USFT    # exclusive
    row_end   = DEM_ORIGIN_N - n_sw           # row just south of tile's N edge (exclusive)
    row_start = row_end - TILE_SIDE_USFT      # northernmost row (inclusive)

    # Skip tiles fully outside the DEM.
    if col_end <= 0 or col_start >= dem_w or row_end <= 0 or row_start >= dem_h:
        return None

    # Clamp to DEM bounds (handles partial tiles at edges).
    ac_rs = max(0, row_start)
    ac_re = min(dem_h, row_end)
    ac_cs = max(0, col_start)
    ac_ce = min(dem_w, col_end)

    sub = dem[ac_rs:ac_re, ac_cs:ac_ce].astype(np.float64)

    # sub[r, c] is at northing (n_sw + (row_end - ac_rs - 1) - r) and easting (e_sw + (ac_cs - col_start) + c).
    # After sub[::-1, :].T, result[col_local, row_local] = sub[row_end - ac_rs - row_local - 1, col_local].
    # Transposing places column → easting axis and (flipped) row → northing axis.
    sub_raster = sub[::-1, :].T   # shape: (easting_width, northing_height)

    # Offsets within the 500×500 output tile.
    dst_e = ac_cs - col_start   # 0 unless tile extends west of DEM
    dst_n = row_end - ac_re     # 0 unless tile extends south of DEM

    full = np.zeros((TILE_SIDE_USFT, TILE_SIDE_USFT), dtype=np.float64)
    h, w = sub_raster.shape[1], sub_raster.shape[0]
    full[dst_e:dst_e + w, dst_n:dst_n + h] = sub_raster

    return np.clip(np.round(full * 12), 0, 65535).astype(np.uint16)


def split_dem(dem_path: str | Path, out_dir: str | Path) -> list[str]:
    """Split a citywide DEM GeoTIFF into 500-usft canonical tiles.

    Args:
        dem_path: Path to the input GeoTIFF.  Must be in EPSG:6539+6360,
            1-usft pixels, heights in US survey feet, with the NW corner at
            (DEM_ORIGIN_E, DEM_ORIGIN_N).
        out_dir: Directory for output .tif and .json tile files.

    Returns:
        Sorted list of tile IDs written.
    """
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    print(f"Reading {dem_path} …")
    dem = tifffile.imread(str(dem_path))
    if dem.ndim == 3:
        dem = dem[0]   # take first band if multi-band

    dem_h, dem_w = dem.shape
    print(f"DEM size: {dem_w} × {dem_h} pixels  "
          f"({dem_w / 5280:.1f} × {dem_h / 5280:.1f} miles)")

    corners = dem_tile_sw_corners(dem_h, dem_w)
    print(f"Candidate tiles: {len(corners)}  →  writing to {out_dir}/")

    tile_ids: list[str] = []
    skipped = 0

    for e_sw, n_sw in tqdm(corners, unit="tile"):
        raster = extract_dem_tile(dem, e_sw, n_sw)
        if raster is None:
            skipped += 1
            continue

        if not np.any(raster):
            skipped += 1
            continue

        tile_id = _tile_id_from_sw_corner(e_sw, n_sw)
        if tile_id is None:
            skipped += 1
            continue  # coordinate outside canonical LAS grid

        tif_path  = out_dir / f"{tile_id}.tif"
        json_path = out_dir / f"{tile_id}.json"

        tifffile.imwrite(str(tif_path), raster)
        json_path.write_text(json.dumps({
            "tile_id": tile_id,
            "x_offset": e_sw,
            "y_offset": n_sw,
            "raster_file": f"{tile_id}.tif",
            "obstruction_ids": [],
        }, indent=2))

        tile_ids.append(tile_id)

    print(f"Done: {len(tile_ids)} tiles written, {skipped} skipped.")
    return sorted(tile_ids)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Split a citywide DEM GeoTIFF into canonical 500-usft tiles"
    )
    parser.add_argument("dem_tif", help="Path to the input DEM GeoTIFF")
    parser.add_argument(
        "out_dir",
        nargs="?",
        default="data/dem_tiles",
        help="Output directory (default: data/dem_tiles)",
    )
    args = parser.parse_args()
    split_dem(args.dem_tif, args.out_dir)


if __name__ == "__main__":
    main()
