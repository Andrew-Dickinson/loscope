"""Parse NYC tax lot CSV into per-block JSON files grouped by borough.

Geometry is already in NYS State Plane Long Island (EPSG:6539).
"""
import csv
import json
from collections import defaultdict
from pathlib import Path

from tqdm import tqdm
from shapely import wkt
from shapely.geometry import mapping

BOROUGH_NAMES = {
    "1": "Manhattan",
    "2": "Bronx",
    "3": "Brooklyn",
    "4": "Queens",
    "5": "Staten Island",
}


def _strip_commas(val: str) -> str:
    return val.replace(",", "").strip()


def parse_row(row: dict) -> dict | None:
    """Parse one CSV row. Returns a lot dict or None if unusable."""
    geom_str = row.get("the_geom", "").strip()
    boro = _strip_commas(row.get("boro", ""))
    block = _strip_commas(row.get("block", ""))
    lot = _strip_commas(row.get("lot", ""))
    bbl = _strip_commas(row.get("bbl", ""))

    if not geom_str or not boro or not block or not lot:
        return None

    borough_name = BOROUGH_NAMES.get(boro)
    if borough_name is None:
        return None

    try:
        geom_nys = wkt.loads(geom_str)
    except Exception:
        return None

    if geom_nys.is_empty:
        return None

    geom_dict = mapping(geom_nys)
    # Round coordinates to nearest integer (1 usft precision matches raster grid)
    geom_dict = _round_coords(geom_dict)

    return {
        "borough": boro,
        "block": block,
        "lot": lot,
        "bbl": bbl,
        "geometry": {
            "type": geom_dict["type"],
            "crs": "EPSG:6539",
            "coordinates": geom_dict["coordinates"],
        },
    }


def _round_coords(geom_dict: dict) -> dict:
    """Round all coordinate values to the nearest integer (1 usft)."""
    def _round_ring(ring):
        return [(_round(x), _round(y)) for x, y in ring]

    def _round(v):
        return round(float(v))

    geom_type = geom_dict["type"]
    coords = geom_dict["coordinates"]

    if geom_type == "Polygon":
        coords = [_round_ring(ring) for ring in coords]
    elif geom_type == "MultiPolygon":
        coords = [[_round_ring(ring) for ring in poly] for poly in coords]

    return {"type": geom_type, "coordinates": coords}


def parse_csv(csv_path: str | Path) -> dict[str, dict[str, dict]]:
    """Parse the full tax lot CSV.

    Returns a nested dict: borough_name -> block -> lot -> lot_dict.
    """
    csv_path = Path(csv_path)
    result: dict[str, dict[str, dict]] = defaultdict(lambda: defaultdict(dict))
    skipped = 0

    with csv_path.open(newline="", encoding="utf-8") as f:
        reader = csv.DictReader(f)
        for _row in tqdm(reader, desc="Parsing lots", unit=" lots"):
            row = {k.lower(): v for k, v in _row.items()}
            lot = parse_row(row)
            if lot is None:
                skipped += 1
                continue
            result[lot["borough"]][lot["block"]][lot["lot"]] = lot

    if skipped:
        import warnings
        warnings.warn(f"Skipped {skipped} rows with missing/invalid data")

    return result


def write_json_files(data: dict[str, dict[str, dict]], out_dir: str | Path) -> int:
    """Write one JSON file per block, grouped into borough sub-directories.

    Returns the total number of files written.
    """
    out_dir = Path(out_dir)
    count = 0

    for borough_name, blocks in sorted(data.items()):
        borough_dir = out_dir / borough_name
        borough_dir.mkdir(parents=True, exist_ok=True)

        for block, lots in sorted(blocks.items(), key=lambda kv: int(kv[0])):
            out_path = borough_dir / f"{block}.json"
            with out_path.open("w", encoding="utf-8") as f:
                json.dump(lots, f, separators=(",", ":"))
            count += 1

    return count
