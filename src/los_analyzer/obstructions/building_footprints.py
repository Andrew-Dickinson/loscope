"""Parse NYC building footprint CSV rows into Obstruction objects.

Each row represents one building polygon in WGS84 lat/lon. The geometry is
projected to NYS State Plane - Long Island (EPSG:6539) and rasterized onto a
1 usft grid. Heights are encoded as uint16 inches above the EPSG:6360 datum.
"""
import csv
import uuid
from pathlib import Path

import numpy as np
import pyproj
import shapely
import shapely.ops
from shapely import wkt
from shapely.geometry import box as shapely_box

from los_analyzer.preprocessing.tile_id import TILE_SIDE_USFT
from los_analyzer.tiles.identify import _tile_id_from_sw_corner
from .model import OBSTRUCTION_TYPE_BUILDING, Obstruction

# Project WGS84 lon/lat (EPSG:4326) -> NYS State Plane horizontal (EPSG:6539)
_TRANSFORMER = pyproj.Transformer.from_crs("EPSG:4326", "EPSG:6539", always_xy=True)


def _project_geometry(geom_wgs84):
    """Reproject a shapely geometry from WGS84 lon/lat to NYS EPSG:6539."""
    return shapely.ops.transform(_TRANSFORMER.transform, geom_wgs84)


def _rasterize(poly_nys, height_inches: int) -> tuple[int, int, np.ndarray]:
    """Rasterize a shapely geometry to a 1 usft uint16 grid.

    Returns (x_sw, y_sw, raster) where raster has shape (W, H) in
    [easting_local, northing_local] axes. Pixels whose center falls inside
    the polygon are set to height_inches; all others are 0.
    """
    minx, miny, maxx, maxy = poly_nys.bounds
    x_sw = int(np.floor(minx))
    y_sw = int(np.floor(miny))
    x_ne = int(np.ceil(maxx))
    y_ne = int(np.ceil(maxy))
    W = max(x_ne - x_sw, 1)
    H = max(y_ne - y_sw, 1)

    # Pixel centers in NYS coords
    xs = np.arange(W, dtype=np.float64) + x_sw + 0.5
    ys = np.arange(H, dtype=np.float64) + y_sw + 0.5
    xx, yy = np.meshgrid(xs, ys, indexing="ij")  # shape (W, H)

    inside = shapely.contains_xy(poly_nys, xx.ravel(), yy.ravel()).reshape(W, H)

    raster = np.zeros((W, H), dtype=np.uint16)
    raster[inside] = np.clip(height_inches, 0, 65535)
    return x_sw, y_sw, raster


def _intersecting_tile_ids(poly_nys) -> list[str]:
    """Return sorted canonical tile IDs for all 500-usft grid cells the polygon touches.

    Checks actual polygon intersection (not just bounding box) against each
    candidate tile square using the canonical grid from identify.py.
    """
    minx, miny, maxx, maxy = poly_nys.bounds
    e_start = int(np.floor(minx / TILE_SIDE_USFT)) * TILE_SIDE_USFT
    n_start = int(np.floor(miny / TILE_SIDE_USFT)) * TILE_SIDE_USFT
    e_end = int(np.floor(maxx / TILE_SIDE_USFT)) * TILE_SIDE_USFT
    n_end = int(np.floor(maxy / TILE_SIDE_USFT)) * TILE_SIDE_USFT

    tile_ids = []
    e = e_start
    while e <= e_end:
        n = n_start
        while n <= n_end:
            tile_square = shapely_box(e, n, e + TILE_SIDE_USFT, n + TILE_SIDE_USFT)
            if poly_nys.intersects(tile_square):
                tid = _tile_id_from_sw_corner(e, n)
                if tid is not None:
                    tile_ids.append(tid)
            n += TILE_SIDE_USFT
        e += TILE_SIDE_USFT
    return sorted(tile_ids)


def parse_building_row(row: dict) -> Obstruction | None:
    """Convert one building footprint CSV row into an Obstruction.

    Returns None if the row has missing or unusable geometry/height data.
    """
    geom_str = row.get("the_geom", "").strip()
    if not geom_str:
        return None

    # Numeric fields may have comma-formatted values (e.g. "1,090,243")
    def _float(val):
        return float(val.replace(",", "").strip())

    try:
        ground_elev = _float(row["Ground Elevation"])
        height_roof = _float(row["Height Roof"])
    except (KeyError, ValueError):
        return None

    total_height_ft = ground_elev + height_roof
    height_inches = int(round(total_height_ft * 12))

    try:
        poly_wgs84 = wkt.loads(geom_str)
    except Exception:
        return None

    poly_nys = _project_geometry(poly_wgs84)
    if poly_nys.is_empty:
        return None

    tile_ids = _intersecting_tile_ids(poly_nys)
    x_sw, y_sw, raster = _rasterize(poly_nys, height_inches)

    # If every pixel is zero the building fell entirely between grid cells
    if not raster.any():
        return None

    def _str(key):
        return row.get(key, "").strip()

    def _int_or_none(key):
        raw = _str(key).replace(",", "")
        return int(raw) if raw.isdigit() else None

    attributes = {
        "BIN": _str("BIN"),
        "BBL": _str("BASE_BBL"),
        "construction_year": _int_or_none("Construction Year"),
        "geometry_source": _str("Geometry Source"),
        "ground_elevation": ground_elev,
        "height_roof": height_roof,
        "last_status_type": _str("LAST_STATUS_TYPE"),
    }

    return Obstruction(
        obstruction_id=str(uuid.uuid4()),
        obstruction_type=OBSTRUCTION_TYPE_BUILDING,
        attributes=attributes,
        x_offset=x_sw,
        y_offset=y_sw,
        raster=raster,
        tile_ids=tile_ids,
    )


def process_csv(csv_path: str | Path, out_dir: str | Path) -> list[str]:
    """Parse a building footprints CSV and write one tif+json pair per building.

    Returns a list of obstruction IDs that were successfully written.
    """
    from .io import save_obstruction

    csv_path = Path(csv_path)
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    written_ids = []
    skipped = 0

    with csv_path.open(newline="", encoding="utf-8") as f:
        reader = csv.DictReader(f)
        for row in reader:
            obs = parse_building_row(row)
            if obs is None:
                skipped += 1
                continue
            save_obstruction(obs, out_dir)
            written_ids.append(obs.obstruction_id)

    if skipped:
        import warnings
        warnings.warn(f"Skipped {skipped} rows with missing/invalid data")

    return written_ids
