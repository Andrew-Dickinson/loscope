"""
Build a SQLite database from NYC DOB CSV exports.

Tables created:
  dob_job_applications       — DOB_Job_Application_Filings (legacy)
  dob_now_job_applications   — DOB_NOW__Build_Job_Application_Filings
  building_footprints        — BUILDING (includes WKT geometry in the_geom)
  tax_lots                   — TAX_LOT_POLYGON (includes WKT geometry in the_geom)
  certificates_of_occupancy  — DOB_NOW Certificate_of_Occupancy

Usage:
  python tools/build_database.py \\
    --dob-jobs      data/permits/DOB_Job_Application_Filings_20260306.csv \\
    --dob-now-jobs  "data/permits/DOB_NOW__Build_–_Job_Application_Filings_20260306.csv" \\
    --footprints    data/new-building-footprints/BUILDING_20260305.csv \\
    --tax-lots      data/tax-lots/TAX_LOT_POLYGON_20260306.csv \\
    --co-issuance   data/co-issuance/DOB_NOW__Certificate_of_Occupancy_20260307.csv \\
    --db            data/nyc_dob.db
"""

import argparse
import re
import sqlite3
import sys
from collections.abc import Callable
from pathlib import Path

import pandas as pd
from tqdm import tqdm

CHUNK_SIZE = 50_000

# ── Borough lookups ──────────────────────────────────────────────────────────

BOROUGH_NAME_TO_NUM: dict[str, int] = {
    'MANHATTAN': 1,
    'BRONX': 2,
    'BROOKLYN': 3,
    'QUEENS': 4,
    'STATEN ISLAND': 5,
}
BOROUGH_NUM_TO_NAME: dict[float, str] = {float(k): v for k, v in {
    1: 'MANHATTAN', 2: 'BRONX', 3: 'BROOKLYN', 4: 'QUEENS', 5: 'STATEN ISLAND',
}.items()}

# ── Date format constants ────────────────────────────────────────────────────

_FMT_MDY       = '%m/%d/%Y'            # "06/19/2019"
_FMT_MDY_TIME  = '%m/%d/%Y %I:%M:%S %p'  # "06/05/2025 06:00:26 PM"
_FMT_mdy_TIME  = '%m/%d/%y %I:%M:%S %p'  # "09/02/25 1:24:22 PM" (CO, 2-digit year)
_FMT_FOOTPRINT = '%Y %b %d %I:%M:%S %p'  # "2025 Aug 22 07:17:38 PM"

# ── Transformation helpers ───────────────────────────────────────────────────

def _to_iso_date(series: pd.Series, fmt: str) -> pd.Series:
    """Parse a date column to ISO YYYY-MM-DD strings; bad values become None."""
    # Collapse any runs of whitespace (handles double-space in CO timestamps)
    s = series.str.strip().str.split().str.join(' ')
    parsed = pd.to_datetime(s, format=fmt, errors='coerce')
    result = parsed.dt.strftime('%Y-%m-%d')
    result[parsed.isna()] = None
    return result


def _to_numeric(series: pd.Series) -> pd.Series:
    """Strip $ and commas, coerce to float."""
    return pd.to_numeric(
        series.str.replace(r'[$,]', '', regex=True).str.strip(),
        errors='coerce',
    )


def _transform_dates(chunk: pd.DataFrame, date_cols: dict[str, str]) -> pd.DataFrame:
    for col, fmt in date_cols.items():
        if col in chunk.columns:
            chunk[col] = _to_iso_date(chunk[col], fmt)
    return chunk


def _transform_numerics(chunk: pd.DataFrame, cols: list[str]) -> pd.DataFrame:
    for col in cols:
        if col in chunk.columns:
            chunk[col] = _to_numeric(chunk[col])
    return chunk


def _add_borough_number(chunk: pd.DataFrame, col: str = 'borough') -> pd.DataFrame:
    """Add borough_number (1–5) from a borough-name column."""
    if col in chunk.columns:
        chunk['borough_number'] = (
            chunk[col].str.upper().str.strip().map(BOROUGH_NAME_TO_NUM)
        )
    return chunk


def _add_borough_name(chunk: pd.DataFrame, col: str = 'boro') -> pd.DataFrame:
    """Add borough_name and borough_number from a boro-number column."""
    if col in chunk.columns:
        nums = pd.to_numeric(chunk[col], errors='coerce')
        chunk['borough_name']   = nums.map(BOROUGH_NUM_TO_NAME)
        chunk['borough_number'] = nums
    return chunk

# ── Per-table transforms ─────────────────────────────────────────────────────

def _transform_co(chunk: pd.DataFrame) -> pd.DataFrame:
    chunk = _transform_dates(chunk, {
        'c_of_o_issuance_date': _FMT_mdy_TIME,
        'submitted_date':       _FMT_MDY,
    })
    chunk = _transform_numerics(chunk, ['number_of_dwelling_units', 'c_of_o_sequence'])
    chunk = _add_borough_number(chunk)
    return chunk


def _transform_dob_jobs(chunk: pd.DataFrame) -> pd.DataFrame:
    chunk = _transform_dates(chunk, {c: _FMT_MDY for c in [
        'pre_filing_date', 'paid', 'fully_paid', 'assigned',
        'approved', 'fully_permitted', 'latest_action_date',
        'signoff_date', 'special_action_date',
    ]})
    chunk = _transform_numerics(chunk, [
        'proposed_height', 'existing_height',
        'proposed_no_of_stories', 'existing_no_of_stories',
        'proposed_dwelling_units', 'existing_dwelling_units',
        'proposed_zoning_sqft', 'existing_zoning_sqft',
        'initial_cost', 'total_est_fee', 'total_construction_floor_area',
    ])
    chunk = _add_borough_number(chunk)
    return chunk


def _transform_dob_now_jobs(chunk: pd.DataFrame) -> pd.DataFrame:
    chunk = _transform_dates(chunk, {c: _FMT_MDY_TIME for c in [
        'filing_date', 'current_status_date', 'first_permit_date',
        'approved_date', 'signoff_date',
    ]})
    chunk = _transform_numerics(chunk, [
        'proposed_height', 'existing_height',
        'proposed_no_of_stories', 'existing_stories',
        'proposed_dwelling_units', 'existing_dwelling_units',
        'initial_cost', 'total_construction_floor_area',
    ])
    chunk = _add_borough_number(chunk)
    return chunk


def _transform_footprints(chunk: pd.DataFrame) -> pd.DataFrame:
    chunk = _transform_dates(chunk, {'last_edited_date': _FMT_FOOTPRINT})
    chunk = _transform_numerics(chunk, [
        'construction_year', 'objectid', 'shape_area',
        'height_roof', 'ground_elevation', 'length',
    ])
    return chunk


def _transform_tax_lots(chunk: pd.DataFrame) -> pd.DataFrame:
    chunk = _transform_numerics(chunk, ['effective_tax_year'])
    chunk = _add_borough_name(chunk, col='boro')
    return chunk


TRANSFORMS: dict[str, Callable[[pd.DataFrame], pd.DataFrame]] = {
    'certificates_of_occupancy': _transform_co,
    'dob_job_applications':      _transform_dob_jobs,
    'dob_now_job_applications':  _transform_dob_now_jobs,
    'building_footprints':       _transform_footprints,
    'tax_lots':                  _transform_tax_lots,
}

# ── Column normalisation ─────────────────────────────────────────────────────

def normalize_column(name: str) -> str:
    """Lowercase, replace non-alphanumeric runs with underscores, strip edges."""
    name = name.strip()
    name = re.sub(r"[^a-zA-Z0-9]+", "_", name)
    name = name.strip("_").lower()
    return name


def load_table(conn: sqlite3.Connection, source: dict) -> None:
    path: Path = source["path"]
    table: str = source["table"]

    if not path.exists():
        print(f"  WARNING: {path} not found — skipping", file=sys.stderr)
        return

    file_size = path.stat().st_size
    print(f"\nLoading {path.name} → {table}")
    print(f"  File size: {file_size / 1_048_576:.1f} MB")

    first_chunk = True
    total_rows = 0

    reader = pd.read_csv(
        path,
        chunksize=CHUNK_SIZE,
        low_memory=False,
        dtype=str,          # keep everything as text to avoid type surprises
        keep_default_na=False,
    )

    transform = TRANSFORMS.get(table)

    for chunk in tqdm(reader, desc=f"  {table}", unit="chunk"):
        chunk.columns = [normalize_column(c) for c in chunk.columns]

        # Deduplicate column names (edge case: two columns normalise to the same name)
        seen: dict[str, int] = {}
        new_cols = []
        for col in chunk.columns:
            if col in seen:
                seen[col] += 1
                new_cols.append(f"{col}_{seen[col]}")
            else:
                seen[col] = 0
                new_cols.append(col)
        chunk.columns = new_cols

        if transform:
            chunk = transform(chunk)

        if_exists = "replace" if first_chunk else "append"
        chunk.to_sql(table, conn, if_exists=if_exists, index=False)
        total_rows += len(chunk)
        first_chunk = False

    print(f"  {total_rows:,} rows loaded")


def add_indexes(conn: sqlite3.Connection) -> None:
    """Add commonly useful indexes for the query phase."""
    indexes = [
        # job applications — join on BBL, BIN, job type
        ("dob_job_applications", "bin"),
        ("dob_job_applications", "block"),
        ("dob_job_applications", "lot"),
        ("dob_job_applications", "borough"),
        ("dob_job_applications", "job_type"),
        # DOB NOW
        ("dob_now_job_applications", "bin"),
        ("dob_now_job_applications", "bbl"),
        ("dob_now_job_applications", "job_type"),
        # footprints — BIN and BBL are primary join keys
        ("building_footprints", "bin"),
        ("building_footprints", "base_bbl"),
        ("building_footprints", "map_pluto_bbl"),
        # tax lots
        ("tax_lots", "bbl"),
        ("tax_lots", "boro"),
        ("tax_lots", "block"),
        ("tax_lots", "lot"),
        # COs
        ("certificates_of_occupancy", "bin"),
        ("certificates_of_occupancy", "bbl"),
        ("certificates_of_occupancy", "job_type"),
    ]

    print("\nCreating indexes…")
    for table, col in indexes:
        idx_name = f"idx_{table}_{col}"
        sql = f"CREATE INDEX IF NOT EXISTS {idx_name} ON {table} ({col})"
        try:
            conn.execute(sql)
        except sqlite3.OperationalError as e:
            # Column may not exist in this export version — skip silently
            print(f"  skipped {idx_name}: {e}")

    conn.commit()
    print("  Done.")


def main() -> None:
    parser = argparse.ArgumentParser(description="Build NYC DOB SQLite database")
    parser.add_argument("--db", default="data/nyc_dob.db",
                        help="Output SQLite database path (default: data/nyc_dob.db)")
    parser.add_argument("--dob-jobs", required=True,
                        metavar="CSV", help="Legacy DOB job application filings CSV")
    parser.add_argument("--dob-now-jobs", required=True,
                        metavar="CSV", help="DOB NOW job application filings CSV")
    parser.add_argument("--footprints", required=True,
                        metavar="CSV", help="Building footprints CSV")
    parser.add_argument("--tax-lots", required=True,
                        metavar="CSV", help="Tax lot polygons CSV")
    parser.add_argument("--co-issuance", required=True,
                        metavar="CSV", help="Certificate of occupancy issuances CSV")
    args = parser.parse_args()

    sources = [
        {"path": Path(args.dob_jobs),     "table": "dob_job_applications"},
        {"path": Path(args.dob_now_jobs), "table": "dob_now_job_applications"},
        {"path": Path(args.footprints),   "table": "building_footprints"},
        {"path": Path(args.tax_lots),     "table": "tax_lots"},
        {"path": Path(args.co_issuance),  "table": "certificates_of_occupancy"},
    ]

    db_path = Path(args.db)
    db_path.parent.mkdir(parents=True, exist_ok=True)

    if db_path.exists():
        print(f"Removing existing database at {db_path}")
        db_path.unlink()

    print(f"Creating database: {db_path}")
    conn = sqlite3.connect(db_path)
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA synchronous=NORMAL")

    for source in sources:
        load_table(conn, source)

    add_indexes(conn)

    # Report final sizes
    print("\nTable row counts:")
    for source in sources:
        table = source["table"]
        try:
            (count,) = conn.execute(f"SELECT COUNT(*) FROM {table}").fetchone()
            print(f"  {table}: {count:,}")
        except sqlite3.OperationalError:
            print(f"  {table}: (not created)")

    conn.close()
    final_size = db_path.stat().st_size
    print(f"\nDatabase written to {db_path} ({final_size / 1_048_576:.1f} MB)")


if __name__ == "__main__":
    main()
