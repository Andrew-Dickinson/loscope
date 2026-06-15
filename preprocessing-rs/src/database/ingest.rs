use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use rusqlite::{Connection, params_from_iter};

use super::normalize::{
    borough_name_to_number, borough_number_to_name, build_bbl, normalize_column_name,
    parse_date, parse_numeric,
};

const BATCH_SIZE: usize = 500;

/// Generic CSV → SQLite loader.
///
/// Reads `csv_path`, normalizes column names, maps them through `col_mapper` to
/// produce SQL column names and values, and inserts into `table` in batches.
/// Rows where `col_mapper` returns `None` are silently skipped.
fn ingest_csv<F>(
    conn: &Connection,
    csv_path: &Path,
    table: &str,
    col_mapper: F,
) -> Result<usize>
where
    F: Fn(&csv::StringRecord, &csv::StringRecord) -> Option<Vec<(String, String)>>,
{
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_path(csv_path)
        .with_context(|| format!("Cannot open {}", csv_path.display()))?;

    let headers = reader.headers()?.clone();
    let norm_headers: Vec<String> = headers.iter().map(normalize_column_name).collect();
    let norm_headers_record: csv::StringRecord = norm_headers.iter().collect();

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    pb.set_message(format!("{table}: 0 rows"));
    pb.enable_steady_tick(Duration::from_millis(100));

    let mut count = 0usize;
    let mut batch: Vec<Vec<(String, String)>> = Vec::with_capacity(BATCH_SIZE);

    conn.execute_batch("BEGIN;")?;
    let result: Result<()> = (|| {
        for result in reader.records() {
            let record = result?;
            if let Some(row) = col_mapper(&record, &norm_headers_record) {
                batch.push(row);
                if batch.len() >= BATCH_SIZE {
                    insert_batch(conn, table, &batch)?;
                    count += batch.len();
                    batch.clear();
                    pb.set_message(format!("{table}: {count} rows"));
                }
            }
        }
        if !batch.is_empty() {
            insert_batch(conn, table, &batch)?;
            count += batch.len();
        }
        Ok(())
    })();

    match result {
        Ok(()) => conn.execute_batch("COMMIT;")?,
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK;");
            pb.finish_with_message(format!("{table}: failed"));
            return Err(e);
        }
    }

    pb.finish_with_message(format!("{table}: {count} rows"));
    Ok(count)
}

fn insert_batch(conn: &Connection, table: &str, batch: &[Vec<(String, String)>]) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let cols: Vec<&str> = batch[0].iter().map(|(k, _)| k.as_str()).collect();
    let placeholders = cols.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let col_list = cols.join(", ");
    let sql = format!("INSERT INTO {table} ({col_list}) VALUES ({placeholders})");
    let mut stmt = conn.prepare_cached(&sql)?;

    for row in batch {
        let vals: Vec<&str> = row.iter().map(|(_, v)| v.as_str()).collect();
        stmt.execute(params_from_iter(vals))?;
    }
    Ok(())
}

fn get_field<'a>(record: &'a csv::StringRecord, headers: &csv::StringRecord, name: &str) -> &'a str {
    headers
        .iter()
        .position(|h| h == name)
        .and_then(|i| record.get(i))
        .unwrap_or("")
}

// ──────────────────────────────────────────────────────────────────────────────
// Per-table ingest functions
// ──────────────────────────────────────────────────────────────────────────────

pub fn ingest_building_footprints(conn: &Connection, csv_path: &Path) -> Result<usize> {
    ingest_csv(conn, csv_path, "building_footprints", |rec, hdrs| {
        let bin = get_field(rec, hdrs, "bin");
        let the_geom = get_field(rec, hdrs, "the_geom");

        let height_roof = parse_numeric(get_field(rec, hdrs, "height_roof"))
            .or_else(|| parse_numeric(get_field(rec, hdrs, "heightroof")))
            .unwrap_or(0.0);
        let ground_elevation = parse_numeric(get_field(rec, hdrs, "ground_elevation"))
            .or_else(|| parse_numeric(get_field(rec, hdrs, "groundelev")))
            .unwrap_or(0.0);
        let construction_year = parse_numeric(get_field(rec, hdrs, "construction_year"))
            .or_else(|| parse_numeric(get_field(rec, hdrs, "cnstrct_yr")))
            .map(|v| v as i64)
            .unwrap_or(0);

        Some(vec![
            ("bin".into(), bin.to_string()),
            ("base_bbl".into(), get_field(rec, hdrs, "base_bbl").to_string()),
            ("mappluto_bbl".into(), get_field(rec, hdrs, "mappluto_bbl").to_string()),
            ("geom_source".into(), get_field(rec, hdrs, "geom_source").to_string()),
            ("the_geom".into(), the_geom.to_string()),
            ("height_roof".into(), height_roof.to_string()),
            ("ground_elevation".into(), ground_elevation.to_string()),
            ("construction_year".into(), construction_year.to_string()),
            ("last_status_type".into(), get_field(rec, hdrs, "last_status_type").to_string()),
            ("last_edited_date".into(), parse_date(get_field(rec, hdrs, "last_edited_date")).unwrap_or_default()),
        ])
    })
}

pub fn ingest_tax_lots(conn: &Connection, csv_path: &Path) -> Result<usize> {
    ingest_csv(conn, csv_path, "tax_lots", |rec, hdrs| {
        let bbl = get_field(rec, hdrs, "bbl");
        let boro_raw = get_field(rec, hdrs, "boro");
        let boro: u8 = boro_raw.trim().parse().ok()?;
        let borough_name = borough_number_to_name(boro).unwrap_or("").to_string();

        Some(vec![
            ("bbl".into(), bbl.to_string()),
            ("boro".into(), boro.to_string()),
            ("borough_name".into(), borough_name),
            ("borough_number".into(), boro.to_string()),
            ("block".into(), get_field(rec, hdrs, "block").to_string()),
            ("lot".into(), get_field(rec, hdrs, "lot").to_string()),
            ("the_geom".into(), get_field(rec, hdrs, "the_geom").to_string()),
            ("last_edited_date".into(), parse_date(get_field(rec, hdrs, "last_edited_date")).unwrap_or_default()),
        ])
    })
}

pub fn ingest_dob_job_applications(conn: &Connection, csv_path: &Path) -> Result<usize> {
    ingest_csv(conn, csv_path, "dob_job_applications", |rec, hdrs| {
        let bin = get_field(rec, hdrs, "bin");
        let bbl_raw = get_field(rec, hdrs, "bbl");
        let borough = get_field(rec, hdrs, "borough");
        let block = get_field(rec, hdrs, "block");
        let lot = get_field(rec, hdrs, "lot");

        let borough_number = borough_name_to_number(borough)
            .map(|n| n.to_string())
            .unwrap_or_default();
        let bbl = if bbl_raw.len() == 10 {
            bbl_raw.to_string()
        } else {
            build_bbl(&borough_number, block, lot).unwrap_or_default()
        };

        Some(vec![
            ("bin".into(), bin.to_string()),
            ("bbl".into(), bbl),
            ("borough".into(), borough.to_string()),
            ("borough_number".into(), borough_number),
            ("block".into(), block.to_string()),
            ("lot".into(), lot.to_string()),
            ("house".into(), get_field(rec, hdrs, "house").to_string()),
            ("street_name".into(), get_field(rec, hdrs, "street_name").to_string()),
            ("job".into(), get_field(rec, hdrs, "job").to_string()),
            ("job_type".into(), get_field(rec, hdrs, "job_type").to_string().to_uppercase()),
            ("pre_filing_date".into(), parse_date(get_field(rec, hdrs, "pre_filing_date")).unwrap_or_default()),
            ("approved".into(), parse_date(get_field(rec, hdrs, "approved")).unwrap_or_default()),
            ("proposed_height".into(), parse_numeric(get_field(rec, hdrs, "proposed_height")).map(|v| v.to_string()).unwrap_or_default()),
            ("existing_height".into(), parse_numeric(get_field(rec, hdrs, "existing_height")).map(|v| v.to_string()).unwrap_or_default()),
        ])
    })
}

pub fn ingest_dob_now_job_applications(conn: &Connection, csv_path: &Path) -> Result<usize> {
    ingest_csv(conn, csv_path, "dob_now_job_applications", |rec, hdrs| {
        let bin = get_field(rec, hdrs, "bin");
        let bbl = get_field(rec, hdrs, "bbl");
        Some(vec![
            ("bin".into(), bin.to_string()),
            ("bbl".into(), bbl.to_string()),
            ("borough".into(), get_field(rec, hdrs, "borough").to_string()),
            ("house_no".into(), get_field(rec, hdrs, "house_no").to_string()),
            ("street_name".into(), get_field(rec, hdrs, "street_name").to_string()),
            ("job_filing_number".into(), get_field(rec, hdrs, "job_filing_number").to_string()),
            ("job_type".into(), get_field(rec, hdrs, "job_type").to_string().to_lowercase()),
            ("filing_date".into(), parse_date(get_field(rec, hdrs, "filing_date")).unwrap_or_default()),
            ("approved_date".into(), parse_date(get_field(rec, hdrs, "approved_date")).unwrap_or_default()),
            ("proposed_height".into(), parse_numeric(get_field(rec, hdrs, "proposed_height")).map(|v| v.to_string()).unwrap_or_default()),
        ])
    })
}

pub fn ingest_certificates_of_occupancy(conn: &Connection, csv_path: &Path) -> Result<usize> {
    ingest_csv(conn, csv_path, "certificates_of_occupancy", |rec, hdrs| {
        let bin = get_field(rec, hdrs, "bin");
        Some(vec![
            ("bin".into(), bin.to_string()),
            ("bbl".into(), get_field(rec, hdrs, "bbl").to_string()),
            ("borough".into(), get_field(rec, hdrs, "borough").to_string()),
            ("house_no".into(), get_field(rec, hdrs, "house_no").to_string()),
            ("street_name".into(), get_field(rec, hdrs, "street_name").to_string()),
            ("job_type".into(), get_field(rec, hdrs, "job_type").to_string().to_lowercase()),
            ("c_of_o_issuance_date".into(), parse_date(get_field(rec, hdrs, "c_of_o_issuance_date")).unwrap_or_default()),
            ("submitted_date".into(), parse_date(get_field(rec, hdrs, "submitted_date")).unwrap_or_default()),
            ("c_of_o_status".into(), get_field(rec, hdrs, "c_of_o_status").to_string()),
        ])
    })
}

pub fn ingest_dob_permit_issuance(conn: &Connection, csv_path: &Path) -> Result<usize> {
    ingest_csv(conn, csv_path, "dob_permit_issuance", |rec, hdrs| {
        let bin = get_field(rec, hdrs, "bin");
        let borough = get_field(rec, hdrs, "borough");
        let block = get_field(rec, hdrs, "block");
        let lot = get_field(rec, hdrs, "lot");
        let borough_num = borough_name_to_number(borough).map(|n| n.to_string()).unwrap_or_default();
        let bbl = build_bbl(&borough_num, block, lot).unwrap_or_default();

        Some(vec![
            ("bin".into(), bin.to_string()),
            ("bbl".into(), bbl),
            ("borough".into(), borough.to_string()),
            ("borough_number".into(), borough_num),
            ("block".into(), block.to_string()),
            ("lot".into(), lot.to_string()),
            ("job".into(), get_field(rec, hdrs, "job").to_string()),
            ("job_type".into(), get_field(rec, hdrs, "job_type").to_string().to_uppercase()),
            ("permit_type".into(), get_field(rec, hdrs, "permit_type").to_string()),
            ("issuance_date".into(), parse_date(get_field(rec, hdrs, "issuance_date")).unwrap_or_default()),
            ("expiration_date".into(), parse_date(get_field(rec, hdrs, "expiration_date")).unwrap_or_default()),
        ])
    })
}

pub fn ingest_dob_now_approved_permits(conn: &Connection, csv_path: &Path) -> Result<usize> {
    ingest_csv(conn, csv_path, "dob_now_approved_permits", |rec, hdrs| {
        let bin = get_field(rec, hdrs, "bin");
        Some(vec![
            ("bin".into(), bin.to_string()),
            ("bbl".into(), get_field(rec, hdrs, "bbl").to_string()),
            ("borough".into(), get_field(rec, hdrs, "borough").to_string()),
            ("job_filing_number".into(), get_field(rec, hdrs, "job_filing_number").to_string()),
            ("work_type".into(), get_field(rec, hdrs, "work_type").to_string()),
            ("issued_date".into(), parse_date(get_field(rec, hdrs, "issued_date")).unwrap_or_default()),
            ("expired_date".into(), parse_date(get_field(rec, hdrs, "expired_date")).unwrap_or_default()),
            ("estimated_job_costs".into(), parse_numeric(get_field(rec, hdrs, "estimated_job_costs")).map(|v| v.to_string()).unwrap_or_default()),
        ])
    })
}

pub fn ingest_condo_units(conn: &Connection, csv_path: &Path) -> Result<usize> {
    ingest_csv(conn, csv_path, "condo_units", |rec, hdrs| {
        let billing_bbl = get_field(rec, hdrs, "condo_billing_bbl");
        let base_bbl = get_field(rec, hdrs, "condo_base_bbl");
        let boro_raw = get_field(rec, hdrs, "condo_base_boro");
        let boro: u8 = boro_raw.trim().parse().unwrap_or(0);
        let borough_name = borough_number_to_name(boro).unwrap_or("").to_string();
        Some(vec![
            ("condo_billing_bbl".into(), billing_bbl.to_string()),
            ("condo_base_bbl".into(), base_bbl.to_string()),
            ("condo_base_boro".into(), boro.to_string()),
            ("condo_base_block".into(), get_field(rec, hdrs, "condo_base_block").to_string()),
            ("condo_base_lot".into(), get_field(rec, hdrs, "condo_base_lot").to_string()),
            ("condo_number".into(), get_field(rec, hdrs, "condo_number").to_string()),
            ("borough_name".into(), borough_name),
            ("borough_number".into(), boro.to_string()),
        ])
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::io::Write;
    use tempfile::tempdir;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(super::super::schema::CREATE_TABLES).unwrap();
        conn
    }

    fn write_csv(path: &Path, content: &str) {
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn ingest_building_footprints_basic() {
        let dir = tempdir().unwrap();
        let csv_path = dir.path().join("footprints.csv");
        write_csv(
            &csv_path,
            "bin,the_geom,height_roof,ground_elevation,construction_year,last_edited_date,base_bbl,mappluto_bbl,geom_source\n\
             1234567,\"POLYGON((0 0,1 0,1 1,0 0))\",100.5,10.0,2022,01/01/2023,3012340001,,GIS\n",
        );

        let conn = in_memory_db();
        let n = ingest_building_footprints(&conn, &csv_path).unwrap();
        assert_eq!(n, 1);

        let (bin, height): (String, f64) = conn
            .query_row("SELECT bin, height_roof FROM building_footprints", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(bin, "1234567");
        assert_eq!(height, 100.5);
    }

    #[test]
    fn ingest_condo_units_basic() {
        let dir = tempdir().unwrap();
        let csv_path = dir.path().join("condos.csv");
        write_csv(&csv_path, "condo_billing_bbl,condo_base_bbl,condo_base_boro,condo_base_block,condo_base_lot,condo_number\n3012340001,3012340000,3,01234,0000,1\n");

        let conn = in_memory_db();
        let n = ingest_condo_units(&conn, &csv_path).unwrap();
        assert_eq!(n, 1);

        let base: String = conn
            .query_row("SELECT condo_base_bbl FROM condo_units", [], |r| r.get(0))
            .unwrap();
        assert_eq!(base, "3012340000");
    }
}
