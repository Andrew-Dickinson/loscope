
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
        proposed_height,
        'DOB_BIS' as application_system
    FROM dob_job_applications
    WHERE job_type = 'nb'
      AND bin IS NOT NULL AND bin != ''
      AND bin NOT IN (1000000, 2000000, 3000000, 4000000, 5000000)
      AND pre_filing_date >= date('now', '-1 year')

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
        proposed_height,
        'DOB_NOW' as application_system
    FROM dob_now_job_applications
    WHERE job_type IN ('new building', 'alt-co - new building with existing elements to remain')
      AND bin IS NOT NULL AND bin != ''
      AND bin NOT IN (1000000, 2000000, 3000000, 4000000, 5000000)
      AND filing_date >= date('now', '-1 year')
),

nb_jobs_deduped AS (
    SELECT job_id, bin, bbl, borough, borough_number, block, lot, house_no, street_name, job_date,
           proposed_height, application_system
    FROM (
        SELECT *, ROW_NUMBER() OVER (PARTITION BY bin ORDER BY job_date DESC) AS rn
        FROM nb_jobs
    )
    WHERE rn = 1
),

-- ── Resolve condo billing BBLs to base BBLs ───────────────────────────────
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
        j.street_name,
        j.job_id,
        j.application_system,
        j.job_date
    FROM nb_jobs_deduped j
    LEFT JOIN condo_units    cu ON cu.condo_billing_bbl = j.bbl
),

-- Latest proposed height — restricted to bins we actually need ────────────────
all_job_heights AS (
    SELECT bin, proposed_height, pre_filing_date AS job_date
    FROM dob_job_applications
    WHERE bin IN (SELECT bin FROM nb_jobs_deduped)
      AND proposed_height IS NOT NULL AND proposed_height != 0

    UNION ALL

    SELECT bin, proposed_height, filing_date AS job_date
    FROM dob_now_job_applications
    WHERE bin IN (SELECT bin FROM nb_jobs_deduped)
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
)

SELECT
    r.bin,
    r.tax_lot_bbl,
    tl.the_geom                                       AS output_geometry,
    null                                              AS ground_elevation,
    lh.proposed_height                                AS height_roof,
    'recent_job_applications'                      AS type,
    json_object(
        'bin',               r.bin,
        'bbl',               r.tax_lot_bbl,
        'ground_elevation',  null,
        'height_roof',       lh.proposed_height,
        'street_addr',       concat(r.house_no, ' ', r.street_name),
        'borough',           r.borough,
        'job_application_system',        r.application_system,
        'job_id',            r.job_id,
        'job_filing_date',   r.job_date
    ) AS props
FROM resolved r
LEFT JOIN tax_lots     tl ON tl.bbl = r.tax_lot_bbl
LEFT JOIN latest_height lh ON lh.bin = r.bin
WHERE output_geometry IS NOT NULL;
