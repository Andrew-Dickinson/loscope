-- New construction buildings: unique BINs with a NEW BUILDING CO issued after 2021-05-01.
--
-- tco_date         : earliest CO issuance date for that BIN (TCO proxy), YYYY-MM-DD
-- proposed_height  : from the most recent job application with a non-zero proposed height
-- tax_lot_geom     : WKT from tax_lots joined on BBL
-- building_geom    : WKT from building_footprints joined on BIN (NULL if not yet present)
--
-- Dates and numeric columns are pre-normalized by build_database.py.

WITH

-- ── 1. New Building COs issued after 2021-05-01, one row per BIN ─────────────
nb_cos AS (
    SELECT
        bin,
        bbl,
        borough,
        borough_number,
        block,
        lot,
        house_no,
        street_name,
        MIN(c_of_o_issuance_date) AS tco_date
    FROM certificates_of_occupancy
    WHERE lower(job_type) LIKE '%new%'
      AND c_of_o_status = 'CO Issued'
      AND c_of_o_issuance_date > '2021-05-01'
    GROUP BY bin
),

-- ── 2. Proposed height from all job applications ──────────────────────────────
all_job_heights AS (
    SELECT bin, proposed_height, pre_filing_date AS job_date
    FROM dob_job_applications
    WHERE bin IS NOT NULL AND bin != '' AND bin NOT in (1000000, 2000000, 3000000, 4000000, 5000000)
      AND proposed_height IS NOT NULL AND proposed_height != 0

    UNION ALL

    SELECT bin, proposed_height, filing_date AS job_date
    FROM dob_now_job_applications
    WHERE bin IS NOT NULL AND bin != '' AND bin NOT in (1000000, 2000000, 3000000, 4000000, 5000000)
      AND proposed_height IS NOT NULL AND proposed_height != 0
),

-- ── 3. Proposed height from most recent filing per BIN ───────────────────────────────────
latest_height AS (
    SELECT bin, proposed_height
    FROM (
        SELECT
            bin,
            proposed_height,
            ROW_NUMBER() OVER (PARTITION BY bin ORDER BY job_date DESC) AS rn
        FROM all_job_heights
    )
    WHERE rn = 1
),

-- ── 4. Resolve condo BBLs ─────────────────────────────────────────────────────
-- If the CO's BBL is a condo billing BBL, replace it with the base BBL(s).
-- One billing BBL mapping to N base BBLs produces N output rows.
-- Non-condo BBLs pass through unchanged via the LEFT JOIN + COALESCE.
resolved AS (
    SELECT
        co.bin,
        co.bbl                              AS billing_bbl,
        COALESCE(cu.condo_base_bbl, co.bbl) AS tax_lot_bbl,
        co.borough,
        co.borough_number,
        co.block,
        co.lot,
        co.house_no,
        co.street_name,
        co.tco_date
    FROM nb_cos co
    LEFT JOIN condo_units cu ON cu.condo_billing_bbl = co.bbl
)

-- ── Final join ───────────────────────────────────────────────────────────────
SELECT
    r.bin,
    r.tax_lot_bbl,
    tl.the_geom                         AS output_geometry,
    null                                AS ground_elevation,
    lh.proposed_height                  AS height_roof,
    'new_construction_certificate_of_occupancy' AS type,
    json_object(
        'bin', r.bin,
        'bbl', r.tax_lot_bbl,
        'ground_elevation', null,
        'height_roof', lh.proposed_height,
        'street_addr', concat(r.house_no, ' ', r.street_name),
        'borough', r.borough,
        'tco_date', r.tco_date
    ) AS props
FROM resolved r
LEFT JOIN tax_lots            tl ON tl.bbl  = r.tax_lot_bbl
LEFT JOIN latest_height       lh ON lh.bin  = r.bin
WHERE output_geometry IS NOT NULL;
