use std::path::Path;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use rusqlite::{Connection, OpenFlags};

/// Export one `{bin}.wkt` file per row in `building_footprints`.
///
/// Each file contains the raw WKT string for that building's footprint polygon.
/// Rows with NULL or empty BIN or geometry are skipped. This replaces the need
/// for the backend to load the full SQLite DB at runtime — each BIN's geometry
/// can be fetched individually via the AssetProvider.
pub fn export_footprint_wkt(db_path: &Path, out_dir: &Path) -> Result<usize> {
    std::fs::create_dir_all(out_dir)?;

    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("Cannot open {}", db_path.display()))?;
    conn.execute_batch("PRAGMA query_only=ON;")?;

    const QUERY: &str = "SELECT bin, the_geom FROM building_footprints \
         WHERE bin IS NOT NULL AND bin != '' \
           AND the_geom IS NOT NULL AND the_geom != ''";

    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM ({QUERY})"),
        [],
        |r| r.get(0),
    )?;

    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{bar:40} {pos}/{len} footprints | {per_sec} | eta: {eta}")
            .unwrap(),
    );

    let mut stmt = conn.prepare(QUERY)?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let count = rows
        .par_iter()
        .map(|(bin, geom)| {
            pb.inc(1);
            let out_path = out_dir.join(format!("{bin}.wkt"));
            match std::fs::write(&out_path, geom) {
                Ok(()) => 1usize,
                Err(e) => {
                    eprintln!("Failed to write {}: {e}", out_path.display());
                    0
                }
            }
        })
        .sum();

    pb.finish_and_clear();
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use tempfile::tempdir;

    fn setup_db_with_footprints(rows: &[(&str, &str)]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE building_footprints (bin TEXT, the_geom TEXT);",
        ).unwrap();
        for (bin, geom) in rows {
            conn.execute(
                "INSERT INTO building_footprints (bin, the_geom) VALUES (?1, ?2)",
                rusqlite::params![bin, geom],
            ).unwrap();
        }
        (dir, db_path)
    }

    #[test]
    fn exports_one_wkt_file_per_row() -> Result<()> {
        let (_dir, db_path) = setup_db_with_footprints(&[
            ("1000001", "POLYGON((0 0,1 0,1 1,0 0))"),
            ("1000002", "POLYGON((2 2,3 2,3 3,2 2))"),
            ("1000003", "POLYGON((4 4,5 4,5 5,4 4))"),
        ]);
        let out_dir = tempdir()?;

        let count = export_footprint_wkt(&db_path, out_dir.path())?;
        assert_eq!(count, 3);

        for bin in ["1000001", "1000002", "1000003"] {
            let wkt_path = out_dir.path().join(format!("{bin}.wkt"));
            assert!(wkt_path.exists(), "{bin}.wkt should exist");
            let content = std::fs::read_to_string(&wkt_path)?;
            assert!(content.starts_with("POLYGON("), "unexpected content: {content}");
        }
        Ok(())
    }

    #[test]
    fn skips_rows_with_empty_bin_or_geom() -> Result<()> {
        let (_dir, db_path) = setup_db_with_footprints(&[
            ("", "POLYGON((0 0,1 0,1 1,0 0))"),        // empty BIN
            ("1000001", ""),                             // empty geom
            ("1000002", "POLYGON((0 0,1 0,1 1,0 0))"), // valid
        ]);
        let out_dir = tempdir()?;

        let count = export_footprint_wkt(&db_path, out_dir.path())?;
        assert_eq!(count, 1);
        assert!(out_dir.path().join("1000002.wkt").exists());
        Ok(())
    }
}
