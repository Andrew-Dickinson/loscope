/// DDL for all tables and indexes in nyc_dob.db, matching build_database.py.
pub const CREATE_TABLES: &str = r#"
CREATE TABLE IF NOT EXISTS building_footprints (
    bin TEXT,
    base_bbl TEXT,
    mappluto_bbl TEXT,
    geom_source TEXT,
    the_geom TEXT,
    height_roof REAL,
    ground_elevation REAL,
    construction_year INTEGER,
    last_edited_date TEXT
);

CREATE TABLE IF NOT EXISTS tax_lots (
    bbl TEXT,
    boro INTEGER,
    borough_name TEXT,
    borough_number INTEGER,
    block TEXT,
    lot TEXT,
    the_geom TEXT,
    effective_tax_year INTEGER,
    objectid INTEGER,
    shape_area REAL,
    shape_length REAL,
    created_date TEXT,
    last_edited_date TEXT
);

CREATE TABLE IF NOT EXISTS dob_job_applications (
    bin TEXT,
    bbl TEXT,
    borough TEXT,
    borough_number INTEGER,
    block TEXT,
    lot TEXT,
    house TEXT,
    street_name TEXT,
    job TEXT,
    job_type TEXT,
    pre_filing_date TEXT,
    paid TEXT,
    fully_paid TEXT,
    assigned TEXT,
    approved TEXT,
    fully_permitted TEXT,
    latest_action_date TEXT,
    signoff_date TEXT,
    special_action_date TEXT,
    proposed_height REAL,
    existing_height REAL,
    proposed_no_of_stories INTEGER,
    existing_no_of_stories INTEGER,
    proposed_dwelling_units INTEGER,
    existing_dwelling_units INTEGER,
    proposed_zoning_sqft REAL,
    existing_zoning_sqft REAL,
    initial_cost REAL,
    total_est_fee REAL,
    total_construction_floor_area REAL
);

CREATE TABLE IF NOT EXISTS dob_now_job_applications (
    bin TEXT,
    bbl TEXT,
    borough TEXT,
    borough_number INTEGER,
    block TEXT,
    lot TEXT,
    house_no TEXT,
    street_name TEXT,
    job_filing_number TEXT,
    job_type TEXT,
    filing_date TEXT,
    current_status_date TEXT,
    first_permit_date TEXT,
    approved_date TEXT,
    signoff_date TEXT,
    proposed_height REAL,
    existing_height REAL,
    proposed_no_of_stories INTEGER,
    existing_stories INTEGER,
    proposed_dwelling_units INTEGER,
    existing_dwelling_units INTEGER,
    initial_cost REAL,
    total_construction_floor_area REAL
);

CREATE TABLE IF NOT EXISTS certificates_of_occupancy (
    bin TEXT,
    bbl TEXT,
    borough TEXT,
    borough_number INTEGER,
    block TEXT,
    lot TEXT,
    house_no TEXT,
    street_name TEXT,
    job_type TEXT,
    c_of_o_issuance_date TEXT,
    submitted_date TEXT,
    number_of_dwelling_units INTEGER,
    c_of_o_sequence INTEGER,
    c_of_o_status TEXT
);

CREATE TABLE IF NOT EXISTS dob_permit_issuance (
    bin TEXT,
    bbl TEXT,
    borough TEXT,
    borough_number INTEGER,
    block TEXT,
    lot TEXT,
    job TEXT,
    job_type TEXT,
    permit_type TEXT,
    filing_date TEXT,
    issuance_date TEXT,
    expiration_date TEXT,
    job_start_date TEXT,
    dobrundate TEXT
);

CREATE TABLE IF NOT EXISTS dob_now_approved_permits (
    bin TEXT,
    bbl TEXT,
    borough TEXT,
    borough_number INTEGER,
    block TEXT,
    lot TEXT,
    job_filing_number TEXT,
    work_type TEXT,
    approved_date TEXT,
    issued_date TEXT,
    expired_date TEXT,
    estimated_job_costs REAL
);

CREATE TABLE IF NOT EXISTS condo_units (
    condo_billing_bbl TEXT,
    condo_base_bbl TEXT,
    condo_base_boro INTEGER,
    condo_base_block TEXT,
    condo_base_lot TEXT,
    condo_number TEXT,
    borough_name TEXT,
    borough_number INTEGER
);
"#;

pub const CREATE_INDEXES: &str = r#"
CREATE INDEX IF NOT EXISTS idx_bf_bin ON building_footprints (bin);
CREATE INDEX IF NOT EXISTS idx_bf_base_bbl ON building_footprints (base_bbl);
CREATE INDEX IF NOT EXISTS idx_bf_mappluto_bbl ON building_footprints (mappluto_bbl);
CREATE INDEX IF NOT EXISTS idx_bf_construction_year ON building_footprints (construction_year);

CREATE INDEX IF NOT EXISTS idx_tl_bbl ON tax_lots (bbl);
CREATE INDEX IF NOT EXISTS idx_tl_boro ON tax_lots (boro);
CREATE INDEX IF NOT EXISTS idx_tl_block ON tax_lots (block);
CREATE INDEX IF NOT EXISTS idx_tl_lot ON tax_lots (lot);

CREATE INDEX IF NOT EXISTS idx_dja_bin ON dob_job_applications (bin);
CREATE INDEX IF NOT EXISTS idx_dja_bbl ON dob_job_applications (bbl);
CREATE INDEX IF NOT EXISTS idx_dja_job_type ON dob_job_applications (job_type);
CREATE INDEX IF NOT EXISTS idx_dja_pre_filing_date ON dob_job_applications (pre_filing_date);
CREATE INDEX IF NOT EXISTS idx_dja_approved ON dob_job_applications (approved);
CREATE INDEX IF NOT EXISTS idx_dja_bin_date ON dob_job_applications (bin, pre_filing_date);

CREATE INDEX IF NOT EXISTS idx_dnj_bin ON dob_now_job_applications (bin);
CREATE INDEX IF NOT EXISTS idx_dnj_bbl ON dob_now_job_applications (bbl);
CREATE INDEX IF NOT EXISTS idx_dnj_job_type ON dob_now_job_applications (job_type);
CREATE INDEX IF NOT EXISTS idx_dnj_filing_date ON dob_now_job_applications (filing_date);
CREATE INDEX IF NOT EXISTS idx_dnj_approved_date ON dob_now_job_applications (approved_date);
CREATE INDEX IF NOT EXISTS idx_dnj_bin_date ON dob_now_job_applications (bin, filing_date);

CREATE INDEX IF NOT EXISTS idx_co_bin ON certificates_of_occupancy (bin);
CREATE INDEX IF NOT EXISTS idx_co_bbl ON certificates_of_occupancy (bbl);
CREATE INDEX IF NOT EXISTS idx_co_job_type ON certificates_of_occupancy (job_type);
CREATE INDEX IF NOT EXISTS idx_co_issuance_date ON certificates_of_occupancy (c_of_o_issuance_date);
CREATE INDEX IF NOT EXISTS idx_co_status ON certificates_of_occupancy (c_of_o_status);
CREATE INDEX IF NOT EXISTS idx_co_type_status_date ON certificates_of_occupancy (job_type, c_of_o_status, c_of_o_issuance_date);

CREATE INDEX IF NOT EXISTS idx_dpi_bin ON dob_permit_issuance (bin);
CREATE INDEX IF NOT EXISTS idx_dpi_bbl ON dob_permit_issuance (bbl);
CREATE INDEX IF NOT EXISTS idx_dpi_job_type ON dob_permit_issuance (job_type);
CREATE INDEX IF NOT EXISTS idx_dpi_issuance_date ON dob_permit_issuance (issuance_date);
CREATE INDEX IF NOT EXISTS idx_dpi_expiration_date ON dob_permit_issuance (expiration_date);

CREATE INDEX IF NOT EXISTS idx_dnap_bin ON dob_now_approved_permits (bin);
CREATE INDEX IF NOT EXISTS idx_dnap_bbl ON dob_now_approved_permits (bbl);
CREATE INDEX IF NOT EXISTS idx_dnap_work_type ON dob_now_approved_permits (work_type);
CREATE INDEX IF NOT EXISTS idx_dnap_issued_date ON dob_now_approved_permits (issued_date);
CREATE INDEX IF NOT EXISTS idx_dnap_expired_date ON dob_now_approved_permits (expired_date);

CREATE INDEX IF NOT EXISTS idx_cu_billing_bbl ON condo_units (condo_billing_bbl);
CREATE INDEX IF NOT EXISTS idx_cu_base_bbl ON condo_units (condo_base_bbl);
"#;
