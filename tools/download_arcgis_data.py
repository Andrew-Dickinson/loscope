"""Download NYC building footprints from ArcGIS Online and save as CSV.

The output CSV mirrors the column layout of BUILDING_20260307.csv used by
build_database.py, with the_geom stored as WKT in EPSG:6539 (the out_sr
requested from the server) rather than WGS84.

Usage:
    python tools/download_arcgis_data.py [--out PATH] [--where CLAUSE] [--chunk N]

No login required — the NYC ArcGIS portal item is public.
"""
from __future__ import annotations

import argparse
import csv
import sys
from pathlib import Path

from arcgis.features import FeatureLayer
from arcgis.gis import GIS
from arcgis.geometry import Geometry
import warnings

from urllib3.exceptions import InsecureRequestWarning

warnings.simplefilter("ignore", InsecureRequestWarning)

PORTAL  = "https://nyc.maps.arcgis.com"
OUT_SR  = 6539   # NAD83(2011) / NY Long Island (US survey feet)

# Map ArcGIS attribute field names (uppercased) → CSV column names
# matching BUILDING_20260307.csv so build_database.py can ingest it as-is.
_FIELD_MAP = {
    "NAME":              "NAME",
    "BIN":               "BIN",
    "DOITT_ID":          "DOITT_ID",
    "SHAPE_AREA":        "SHAPE_AREA",
    "BASE_BBL":          "BASE_BBL",
    "OBJECTID":          "OBJECTID",
    "CONSTRUCTION_YEAR": "Construction Year",
    "FEATURE_CODE":      "Feature Code",
    "GEOM_SOURCE":       "Geometry Source",
    "GROUND_ELEVATION":  "Ground Elevation",
    "HEIGHT_ROOF":       "Height Roof",
    "LAST_EDITED_DATE":  "LAST_EDITED_DATE",
    "LAST_STATUS_TYPE":  "LAST_STATUS_TYPE",
    "MAPPLUTO_BBL":      "Map Pluto BBL",
    "SHAPE_LENGTH":      "Length",
}


def _iter_chunks(feature_layer, where: str, chunk_size: int):
    """Yield Feature objects in pages to avoid server result-set limits."""
    offset = 0
    while True:
        fs = feature_layer.query(
            where=where,
            out_fields="*",
            out_sr=OUT_SR,
            result_offset=offset,
            result_record_count=chunk_size,
            return_geometry=True,
        )
        features = fs.features
        if not features:
            break
        yield from features
        if len(features) < chunk_size:
            break
        offset += len(features)


def download(src_url: str, out_path: Path, where: str = "1=1", chunk_size: int = 1000) -> None:
    from tqdm import tqdm

    print(f"Connecting to {PORTAL} …")
    gis = GIS(PORTAL)

    print(f"Fetching item {src_url} …")
    feature_layer = FeatureLayer(src_url, gis=gis)
    if feature_layer is None:
        sys.exit(f"Url {src_url!r} not found.")

    print(f"Layer: {feature_layer.properties.name}")

    total = feature_layer.query(where=where, return_count_only=True)
    print(f"Total features: {total:,}")

    out_path.parent.mkdir(parents=True, exist_ok=True)

    written = 0
    with out_path.open("w", newline="", encoding="utf-8") as f:
        writer = None

        with tqdm(total=total, unit="feat", desc="Downloading") as pbar:
            for feat in _iter_chunks(feature_layer, where, chunk_size):
                attrs = {k.upper(): v for k, v in feat.attributes.items()}

                # Build row: geometry WKT first, then mapped attribute columns
                row: dict[str, object] = {}
                row["the_geom"] = Geometry(feat.geometry).WKT if feat.geometry else ""

                for src_field, dst_col in _FIELD_MAP.items():
                    row[dst_col] = attrs.get(src_field, "")

                if writer is None:
                    writer = csv.DictWriter(f, fieldnames=list(row.keys()))
                    writer.writeheader()

                writer.writerow(row)
                written += 1
                pbar.update(1)

    print(f"Done. {written:,} features written to {out_path}")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Download Dataset from ArcGIS Online to CSV"
    )
    parser.add_argument(
        "url",
        help="Source URL, like https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/BUILDING_view/FeatureServer/0",
    )
    parser.add_argument(
        "--out",
        default="data/building-footprints/arcgis_download.csv",
        metavar="PATH",
        help="Output CSV path (default: data/building-footprints/arcgis_download.csv)",
    )
    parser.add_argument(
        "--where",
        default="1=1",
        metavar="SQL",
        help="Server-side WHERE clause to filter features (default: 1=1)",
    )
    parser.add_argument(
        "--chunk",
        type=int,
        default=1000,
        metavar="N",
        help="Features per page request (default: 1000)",
    )
    args = parser.parse_args()
    download(args.url, Path(args.out), where=args.where, chunk_size=args.chunk)


if __name__ == "__main__":
    main()
