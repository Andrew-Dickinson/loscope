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
    WHERE job_type = 'NEW BUILDING'
      AND c_of_o_status = 'CO Issued'
      AND c_of_o_issuance_date > '2021-05-01'
    GROUP BY bin
),

-- ── 2. Proposed height from all job applications ──────────────────────────────
all_job_heights AS (
    SELECT bin, proposed_height, latest_action_date AS job_date
    FROM dob_job_applications
    WHERE bin IS NOT NULL AND bin != ''
      AND proposed_height IS NOT NULL AND proposed_height != 0

    UNION ALL

    SELECT bin, proposed_height, current_status_date AS job_date
    FROM dob_now_job_applications
    WHERE bin IS NOT NULL AND bin != ''
      AND proposed_height IS NOT NULL AND proposed_height != 0
),

-- ── 3. Most recent proposed height per BIN ───────────────────────────────────
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
)

-- ── Final join ───────────────────────────────────────────────────────────────
SELECT
    co.bin,
    co.bbl,
    co.borough,
    co.borough_number,
    co.block,
    co.lot,
    co.house_no,
    co.street_name,
    co.tco_date,
    lh.proposed_height                  AS proposed_height_ft,
    tl.the_geom                         AS tax_lot_geom,
    bf.the_geom                         AS building_geom
FROM nb_cos co
LEFT JOIN tax_lots            tl ON tl.bbl  = co.bbl
LEFT JOIN building_footprints bf ON bf.bin  = co.bin
LEFT JOIN latest_height       lh ON lh.bin  = co.bin
ORDER BY co.tco_date;
