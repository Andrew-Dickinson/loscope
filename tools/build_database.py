"""
Build a SQLite database from NYC DOB CSV exports.

Tables created:
  dob_job_applications       — DOB_Job_Application_Filings (legacy)
  dob_now_job_applications   — DOB_NOW__Build_Job_Application_Filings
  building_footprints        — BUILDING (includes WKT geometry in the_geom)
  tax_lots                   — TAX_LOT_POLYGON (includes WKT geometry in the_geom)
  certificates_of_occupancy  — DOB_NOW Certificate_of_Occupancy
  dob_now_approved_permits   — DOB_NOW__Build_Approved_Permits
  dob_permit_issuance        — DOB_Permit_Issuance (legacy)

Usage:
  python tools/build_database.py \\
    --dob-jobs          data/permits/DOB_Job_Application_Filings_20260306.csv \\
    --dob-now-jobs      "data/permits/DOB_NOW__Build_–_Job_Application_Filings_20260306.csv" \\
    --footprints        data/new-building-footprints/BUILDING_20260305.csv \\
    --tax-lots          data/tax-lots/TAX_LOT_POLYGON_20260306.csv \\
    --co-issuance       data/co-issuance/DOB_NOW__Certificate_of_Occupancy_20260307.csv \\
    --dob-now-permits   "data/permit-issuance/DOB_NOW__Build_–_Approved_Permits_20260307.csv" \\
    --dob-permits       data/permit-issuance/DOB_Permit_Issuance_20260307.csv \\
    --db                data/nyc_dob.db
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
_FMT_MDY_HMS   = '%m/%d/%Y %H:%M:%S'     # "05/11/2022 00:00:00" (24-hour, DOBRunDate)
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


def _strip_commas(chunk: pd.DataFrame, cols: list[str]) -> pd.DataFrame:
    """Remove commas from string identifier columns (e.g. block, lot) without type conversion."""
    for col in cols:
        if col in chunk.columns:
            chunk[col] = chunk[col].str.replace(',', '', regex=False).str.strip()
    return chunk


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


def _fill_bbl(chunk: pd.DataFrame) -> pd.DataFrame:
    """Bidirectionally fill BBL ↔ (borough_number, block, lot).

    BBL format: 1-digit boro + 5-digit block (zero-padded) + 4-digit lot (zero-padded).

    Pass 1 — BBL → components: where BBL is present but borough_number / block / lot
              are missing, parse them out of the BBL string.
    Pass 2 — components → BBL: where all three components are present but BBL is
              missing, construct and zero-pad the BBL.
    """
    def _is_blank(s: pd.Series) -> pd.Series:
        return s.isna() | (s.astype(str).str.strip() == '')

    # ── Pass 1: BBL → components ─────────────────────────────────────────────
    if 'bbl' in chunk.columns:
        has_bbl = ~_is_blank(chunk['bbl'])
        if has_bbl.any():
            bbl_str = chunk.loc[has_bbl, 'bbl'].astype(str).str.strip().str.zfill(10)

            parsed_boro  = pd.to_numeric(bbl_str.str[0:1],  errors='coerce')
            parsed_block = pd.to_numeric(bbl_str.str[1:6],  errors='coerce')
            parsed_lot   = pd.to_numeric(bbl_str.str[6:10], errors='coerce')

            for parsed, col in [
                (parsed_boro,  'borough_number'),
                (parsed_block, 'block'),
                (parsed_lot,   'lot'),
            ]:
                if col not in chunk.columns:
                    chunk[col] = None
                missing = _is_blank(chunk[col])
                fill_mask = has_bbl & missing & parsed.notna()
                if not fill_mask.any():
                    continue
                values = parsed[fill_mask]
                # block/lot are StringDtype columns — must receive strings, not floats
                if col in ('block', 'lot'):
                    values = values.astype(int).astype(str)
                chunk.loc[fill_mask, col] = values

            # Also derive borough name if we resolved a borough number
            if 'borough' in chunk.columns:
                boro_nums = pd.to_numeric(chunk['borough_number'], errors='coerce')
                missing_borough = _is_blank(chunk['borough'])
                chunk.loc[missing_borough, 'borough'] = (
                    boro_nums[missing_borough].map(BOROUGH_NUM_TO_NAME)
                )

    # ── Pass 2: components → BBL ─────────────────────────────────────────────
    if not all(c in chunk.columns for c in ('borough_number', 'block', 'lot')):
        return chunk

    boro  = pd.to_numeric(chunk['borough_number'], errors='coerce')
    block = pd.to_numeric(chunk['block'],          errors='coerce')
    lot   = pd.to_numeric(chunk['lot'],            errors='coerce')
    valid = boro.notna() & block.notna() & lot.notna()

    computed = (
        boro[valid].astype(int).astype(str)
        + block[valid].astype(int).astype(str).str.zfill(5)
        + lot[valid].astype(int).astype(str).str.zfill(4)
    )

    if 'bbl' not in chunk.columns:
        chunk['bbl'] = None
        chunk.loc[valid, 'bbl'] = computed
    else:
        missing = _is_blank(chunk['bbl'])
        chunk.loc[valid & missing, 'bbl'] = computed[valid & missing]

    return chunk

# ── Per-table transforms ─────────────────────────────────────────────────────

def _transform_co(chunk: pd.DataFrame) -> pd.DataFrame:
    chunk = _strip_commas(chunk, ['block', 'lot'])
    chunk = _transform_dates(chunk, {
        'c_of_o_issuance_date': _FMT_mdy_TIME,
        'submitted_date':       _FMT_MDY,
    })
    chunk = _transform_numerics(chunk, ['number_of_dwelling_units', 'c_of_o_sequence'])
    chunk = _add_borough_number(chunk)
    chunk = _fill_bbl(chunk)
    return chunk


def _transform_dob_jobs(chunk: pd.DataFrame) -> pd.DataFrame:
    chunk = _strip_commas(chunk, ['block', 'lot'])
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
    chunk = _fill_bbl(chunk)
    return chunk


def _transform_dob_now_jobs(chunk: pd.DataFrame) -> pd.DataFrame:
    chunk = _strip_commas(chunk, ['block', 'lot'])
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
    chunk = _fill_bbl(chunk)
    return chunk


def _transform_footprints(chunk: pd.DataFrame) -> pd.DataFrame:
    chunk = _transform_dates(chunk, {'last_edited_date': _FMT_FOOTPRINT})
    chunk = _transform_numerics(chunk, [
        'construction_year', 'objectid', 'shape_area',
        'height_roof', 'ground_elevation', 'length',
    ])
    return chunk


def _transform_tax_lots(chunk: pd.DataFrame) -> pd.DataFrame:
    chunk = _strip_commas(chunk, ['block', 'lot'])
    chunk = _transform_numerics(chunk, ['effective_tax_year'])
    chunk = _add_borough_name(chunk, col='boro')
    return chunk


def _transform_condos(chunk: pd.DataFrame) -> pd.DataFrame:
    chunk = _strip_commas(chunk, ['condo_base_block', 'condo_base_lot', 'condo_number'])
    chunk = _add_borough_name(chunk, col='condo_base_boro')
    return chunk


def _transform_dob_now_permits(chunk: pd.DataFrame) -> pd.DataFrame:
    """DOB_NOW__Build_–_Approved_Permits"""
    chunk = _strip_commas(chunk, ['block', 'lot'])
    chunk = _transform_dates(chunk, {c: _FMT_FOOTPRINT for c in [
        'approved_date', 'issued_date', 'expired_date',
    ]})
    chunk = _transform_numerics(chunk, ['estimated_job_costs'])
    chunk = _add_borough_number(chunk)
    chunk = _fill_bbl(chunk)
    return chunk


def _transform_dob_permit_issuance(chunk: pd.DataFrame) -> pd.DataFrame:
    """DOB_Permit_Issuance (legacy)"""
    chunk = _strip_commas(chunk, ['block', 'lot'])
    chunk = _transform_dates(chunk, {c: _FMT_MDY for c in [
        'filing_date', 'issuance_date', 'expiration_date', 'job_start_date',
    ]})
    chunk = _transform_dates(chunk, {'dobrundate': _FMT_MDY_HMS})
    chunk = _add_borough_number(chunk)
    chunk = _fill_bbl(chunk)
    return chunk


TRANSFORMS: dict[str, Callable[[pd.DataFrame], pd.DataFrame]] = {
    'certificates_of_occupancy': _transform_co,
    'dob_job_applications':      _transform_dob_jobs,
    'dob_now_job_applications':  _transform_dob_now_jobs,
    'building_footprints':       _transform_footprints,
    'tax_lots':                  _transform_tax_lots,
    'condo_units':               _transform_condos,
    'dob_now_approved_permits':  _transform_dob_now_permits,
    'dob_permit_issuance':       _transform_dob_permit_issuance,
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
        # job applications — join on BBL, BIN, job type, job number
        ("dob_job_applications", "bin"),
        ("dob_job_applications", "block"),
        ("dob_job_applications", "lot"),
        ("dob_job_applications", "borough"),
        ("dob_job_applications", "job_type"),
        ("dob_job_applications", "job"),
        # date columns used in WHERE filters (recent_job_applications, approved_job_applications)
        ("dob_job_applications", "pre_filing_date"),
        ("dob_job_applications", "approved"),
        # DOB NOW job applications
        ("dob_now_job_applications", "bin"),
        ("dob_now_job_applications", "bbl"),
        ("dob_now_job_applications", "job_type"),
        ("dob_now_job_applications", "job_filing_number"),
        # date columns used in WHERE filters
        ("dob_now_job_applications", "filing_date"),
        ("dob_now_job_applications", "approved_date"),
        # footprints — BIN and BBL are primary join keys
        ("building_footprints", "bin"),
        ("building_footprints", "base_bbl"),
        ("building_footprints", "map_pluto_bbl"),
        # construction_year used for range filter in new_construction_footprints.sql
        ("building_footprints", "construction_year"),
        # tax lots
        ("tax_lots", "bbl"),
        ("tax_lots", "boro"),
        ("tax_lots", "block"),
        ("tax_lots", "lot"),
        # COs
        ("certificates_of_occupancy", "bin"),
        ("certificates_of_occupancy", "bbl"),
        ("certificates_of_occupancy", "job_type"),
        # date and status filters in new_construction_co.sql
        ("certificates_of_occupancy", "c_of_o_issuance_date"),
        ("certificates_of_occupancy", "c_of_o_status"),
        # condo unit → base lot mapping
        ("condo_units", "condo_billing_bbl"),
        ("condo_units", "condo_base_bbl"),
        # DOB NOW approved permits
        ("dob_now_approved_permits", "bin"),
        ("dob_now_approved_permits", "bbl"),
        ("dob_now_approved_permits", "borough"),
        ("dob_now_approved_permits", "job_filing_number"),
        ("dob_now_approved_permits", "work_type"),
        ("dob_now_approved_permits", "issued_date"),
        ("dob_now_approved_permits", "expired_date"),
        # legacy DOB permit issuance
        ("dob_permit_issuance", "bin"),
        ("dob_permit_issuance", "block"),
        ("dob_permit_issuance", "lot"),
        ("dob_permit_issuance", "borough"),
        ("dob_permit_issuance", "job_type"),
        ("dob_permit_issuance", "permit_type"),
        ("dob_permit_issuance", "job"),
        ("dob_permit_issuance", "expiration_date"),
    ]

    # Expression indexes and composite indexes that the simple (table, col) format can't express.
    # Expression indexes on lower(job_type) are needed because every query filters with
    # lower(job_type) = '...' — a plain job_type index is not used for such expressions.
    # Composite indexes on (bin, date) cover the all_job_heights window function
    # (PARTITION BY bin ORDER BY job_date DESC) with a single index scan.
    extra_indexes: list[tuple[str, str]] = [
        # lower(job_type) expression indexes
        ("idx_dob_job_applications_job_type_lower",
         "CREATE INDEX IF NOT EXISTS idx_dob_job_applications_job_type_lower"
         " ON dob_job_applications (lower(job_type))"),
        ("idx_dob_now_job_applications_job_type_lower",
         "CREATE INDEX IF NOT EXISTS idx_dob_now_job_applications_job_type_lower"
         " ON dob_now_job_applications (lower(job_type))"),
        ("idx_certificates_of_occupancy_job_type_lower",
         "CREATE INDEX IF NOT EXISTS idx_certificates_of_occupancy_job_type_lower"
         " ON certificates_of_occupancy (lower(job_type))"),
        # Composite indexes for all_job_heights PARTITION BY bin ORDER BY job_date DESC
        ("idx_dob_job_applications_bin_pre_filing_date",
         "CREATE INDEX IF NOT EXISTS idx_dob_job_applications_bin_pre_filing_date"
         " ON dob_job_applications (bin, pre_filing_date)"),
        ("idx_dob_now_job_applications_bin_filing_date",
         "CREATE INDEX IF NOT EXISTS idx_dob_now_job_applications_bin_filing_date"
         " ON dob_now_job_applications (bin, filing_date)"),
        # Partial indexes for all_job_heights: only rows with a valid proposed_height,
        # covering (bin, date) for the window function.
        ("idx_dob_job_applications_height_bin_date",
         "CREATE INDEX IF NOT EXISTS idx_dob_job_applications_height_bin_date"
         " ON dob_job_applications (bin, pre_filing_date)"
         " WHERE proposed_height IS NOT NULL AND proposed_height != 0"),
        ("idx_dob_now_job_applications_height_bin_date",
         "CREATE INDEX IF NOT EXISTS idx_dob_now_job_applications_height_bin_date"
         " ON dob_now_job_applications (bin, filing_date)"
         " WHERE proposed_height IS NOT NULL AND proposed_height != 0"),
        # Composite index for new_construction_co.sql: job_type IN (...) + status + date range
        ("idx_certificates_of_occupancy_job_type_status_date",
         "CREATE INDEX IF NOT EXISTS idx_certificates_of_occupancy_job_type_status_date"
         " ON certificates_of_occupancy (lower(job_type), c_of_o_status, c_of_o_issuance_date)"),
        # Composite index for active_permits.sql: job_type equality + expiration range
        ("idx_dob_permit_issuance_job_type_expiration",
         "CREATE INDEX IF NOT EXISTS idx_dob_permit_issuance_job_type_expiration"
         " ON dob_permit_issuance (lower(job_type), expiration_date)"),
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

    for idx_name, sql in extra_indexes:
        try:
            conn.execute(sql)
        except sqlite3.OperationalError as e:
            print(f"  skipped {idx_name}: {e}")

    conn.commit()
    print("  Done.")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Build (or incrementally update) the NYC DOB SQLite database. "
                    "Only tables whose CSV flags are provided are replaced; "
                    "omitted tables are left untouched in an existing database.",
    )
    parser.add_argument("--db", default="data/nyc_dob.db",
                        help="SQLite database path (default: data/nyc_dob.db)")
    parser.add_argument("--dob-jobs",    metavar="CSV", help="Legacy DOB job application filings CSV")
    parser.add_argument("--dob-now-jobs",metavar="CSV", help="DOB NOW job application filings CSV")
    parser.add_argument("--footprints",  metavar="CSV", help="Building footprints CSV")
    parser.add_argument("--tax-lots",    metavar="CSV", help="Tax lot polygons CSV")
    parser.add_argument("--condos",           metavar="CSV", help="Condo unit → base BBL mapping CSV")
    parser.add_argument("--co-issuance",      metavar="CSV", help="Certificate of occupancy issuances CSV")
    parser.add_argument("--dob-now-permits",  metavar="CSV", help="DOB NOW approved permits CSV")
    parser.add_argument("--dob-permits",      metavar="CSV", help="Legacy DOB permit issuance CSV")
    args = parser.parse_args()

    # Build the list of sources from whichever args were actually supplied
    arg_map = [
        (args.dob_jobs,          "dob_job_applications"),
        (args.dob_now_jobs,      "dob_now_job_applications"),
        (args.footprints,        "building_footprints"),
        (args.tax_lots,          "tax_lots"),
        (args.condos,            "condo_units"),
        (args.co_issuance,       "certificates_of_occupancy"),
        (args.dob_now_permits,   "dob_now_approved_permits"),
        (args.dob_permits,       "dob_permit_issuance"),
    ]
    sources = [
        {"path": Path(path), "table": table}
        for path, table in arg_map
        if path is not None
    ]

    db_path = Path(args.db)
    db_path.parent.mkdir(parents=True, exist_ok=True)

    if db_path.exists():
        print(f"Updating existing database: {db_path}")
    else:
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
