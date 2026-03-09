
WITH

nb_jobs AS (
    SELECT
        job as job_id,
        bin,
        bbl,
        borough,
        borough_number,
        block,
        lot,
        house       AS house_no,
        street_name,
        pre_filing_date AS job_date,
        proposed_height
    FROM dob_job_applications
    WHERE lower(job_type) = 'nb'
      AND bin IS NOT NULL AND bin != ''
      AND bin NOT IN (1000000, 2000000, 3000000, 4000000, 5000000)
      AND pre_filing_date >= '2025-03-08'

    UNION ALL

    SELECT
        job_filing_number as job_id,
        bin,
        bbl,
        borough,
        borough_number,
        block,
        lot,
        house_no,
        street_name,
        filing_date AS job_date,
        proposed_height
    FROM dob_now_job_applications
    WHERE lower(job_type) LIKE '%new building%'
      AND bin IS NOT NULL AND bin != ''
      AND bin NOT IN (1000000, 2000000, 3000000, 4000000, 5000000)
      AND filing_date >= '2025-03-08'
),

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

-- ── 6. Resolve condo billing BBLs to base BBLs ───────────────────────────────
resolved AS (
    SELECT
        j.bin,
        j.bbl                               AS billing_bbl,
        COALESCE(cu.condo_base_bbl, j.bbl)  AS tax_lot_bbl,
        j.borough,
        j.borough_number,
        j.block,
        j.lot,
        j.house_no,
        j.street_name
    FROM nb_jobs j
    LEFT JOIN condo_units    cu ON cu.condo_billing_bbl = j.bbl
)

SELECT
    r.bin,
    r.tax_lot_bbl,
    tl.the_geom                                       AS output_geometry,
    null                                              AS ground_elevation,
    lh.proposed_height                                AS height_roof,
    'new_building_application_filed_in_last_year'  AS type,
    json_object(
        'bin',               r.bin,
        'bbl',               r.tax_lot_bbl,
        'ground_elevation',  null,
        'height_roof',       lh.proposed_height,
        'street_addr',       concat(r.house_no, ' ', r.street_name),
        'borough',           r.borough
    ) AS props
FROM resolved r
LEFT JOIN tax_lots     tl ON tl.bbl = r.tax_lot_bbl
LEFT JOIN latest_height lh ON lh.bin = r.bin
WHERE output_geometry IS NOT NULL;
