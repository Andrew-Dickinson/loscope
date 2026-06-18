use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::blocking::Client;

const SOCRATA_BASE: &str = "https://data.cityofnewyork.us/resource";
const SOCRATA_EXPORT_BASE: &str = "https://data.cityofnewyork.us/api/v3/views";

/// Download a Socrata dataset as a single bulk CSV export via POST.
///
/// Socrata exposes `/api/v3/views/{id}/export.csv` which returns the full
/// dataset in one streaming response (gzip-compressed). This is orders of
/// magnitude faster than the paginated SoQL approach because the server can
/// stream a pre-built snapshot rather than executing repeated queries.
///
/// The JSON body `{"serializationOptions":{"defaultGroupSeparator":",","defaultDecimalSeparator":"."}}`
/// matches what the NYC Open Data web UI sends.
pub fn download_bulk(dataset_id: &str, out_path: &Path) -> Result<()> {
    const MAX_RETRIES: u32 = 7;

    let client = Client::builder()
        .build()
        .context("Failed to build HTTP client")?;

    let cache_bust: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let url = format!(
        "{SOCRATA_EXPORT_BASE}/{dataset_id}/export.csv?cacheBust={cache_bust}&accessType=DOWNLOAD"
    );

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner} {bytes} downloaded ({bytes_per_sec})")
            .unwrap(),
    );

    let mut last_err: anyhow::Error = anyhow::anyhow!("no attempts made");

    for attempt in 0..=MAX_RETRIES {
        if attempt > 0 {
            let delay = Duration::from_secs(1 << (attempt - 1));
            eprintln!("{last_err}, retrying in {delay:?}…");
            std::thread::sleep(delay);
            pb.set_position(0);
        }

        let resp = match client
            .post(&url)
            .header("Accept", "text/csv")
            .header("Content-Type", "application/json")
            .body(r#"{"serializationOptions":{"defaultGroupSeparator":",","defaultDecimalSeparator":"."}}"#)
            .send()
        {
            Err(e) => { last_err = anyhow::Error::new(e); continue; }
            Ok(r) => r,
        };

        let status = resp.status();
        if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            last_err = anyhow::anyhow!("server returned {status}");
            continue;
        }
        if let Err(e) = resp.error_for_status_ref() {
            return Err(e).with_context(|| format!("Server returned error for {url}"));
        }

        // Truncate/create the file fresh on each attempt so a partial write
        // from a previous attempt doesn't corrupt the output.
        let mut out = std::io::BufWriter::new(
            std::fs::File::create(out_path)
                .with_context(|| format!("Cannot create {}", out_path.display()))?,
        );

        let mut bytes_written: u64 = 0;
        let mut reader = BufReader::new(resp);
        let mut buf = [0u8; 65536];
        let stream_err = loop {
            match std::io::Read::read(&mut reader, &mut buf) {
                Err(e) => break Some(e),
                Ok(0) => break None,
                Ok(n) => {
                    if let Err(e) = out.write_all(&buf[..n]) {
                        return Err(e).context("Error writing output file");
                    }
                    bytes_written += n as u64;
                    pb.set_position(bytes_written);
                }
            }
        };

        if let Some(e) = stream_err {
            last_err = anyhow::Error::new(e).context("Error reading response body");
            continue;
        }

        pb.finish_and_clear();
        println!("Wrote {bytes_written} bytes to {}", out_path.display());
        return Ok(());
    }

    pb.finish_and_clear();
    Err(last_err).with_context(|| format!("Failed to download {url} after {MAX_RETRIES} retries"))
}

/// Download a Socrata dataset as CSV using offset-based pagination.
///
/// Writes the full CSV (including header) to `out_path`. Subsequent pages have
/// their header row stripped so the output is a single valid CSV.
pub fn download(
    dataset_id: &str,
    out_path: &Path,
    chunk_size: usize,
) -> Result<()> {
    let client = Client::builder()
        .build()
        .context("Failed to build HTTP client")?;

    // Get total count first.
    let count_url = format!("{SOCRATA_BASE}/{dataset_id}.json?$select=count(*)");
    let count_resp: serde_json::Value = client
        .get(&count_url)
        .send()
        .context("Failed to fetch row count")?
        .json()
        .context("Failed to parse row count")?;
    let total = count_resp[0]["count(*)"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{bar:40} {pos}/{len} rows | {per_sec} | eta: {eta}")
            .unwrap(),
    );

    let mut out = std::io::BufWriter::new(
        std::fs::File::create(out_path)
            .with_context(|| format!("Cannot create {}", out_path.display()))?,
    );

    let mut offset = 0usize;
    let mut wrote_header = false;

    loop {
        let page_url = format!(
            "{SOCRATA_BASE}/{dataset_id}.csv?$limit={chunk_size}&$offset={offset}"
        );
        let resp = client
            .get(&page_url)
            .send()
            .with_context(|| format!("Failed to fetch rows at offset {offset}"))?;

        let body = resp.text().context("Failed to read page body")?;
        let mut lines = BufReader::new(body.as_bytes()).lines();

        // First line is the header.
        let header = match lines.next() {
            Some(Ok(h)) => h,
            _ => break,
        };

        if !wrote_header {
            writeln!(out, "{header}")?;
            wrote_header = true;
        }

        let mut rows_in_page = 0usize;
        for line in lines {
            let line = line?;
            writeln!(out, "{line}")?;
            rows_in_page += 1;
        }

        pb.inc(rows_in_page as u64);
        offset += rows_in_page;

        if rows_in_page < chunk_size {
            break;
        }
    }

    pb.finish_and_clear();
    println!("Wrote {} rows to {}", offset, out_path.display());
    Ok(())
}

/// Hardcoded Socrata dataset registry for NYC Open Data.
pub struct SocrataDataset {
    pub id: &'static str,
    pub filename: &'static str,
    pub description: &'static str,
}

pub const NYC_OPEN_DATA_DATASETS: &[SocrataDataset] = &[
    SocrataDataset {
        id: "pkdm-hqz6",
        filename: "DOB-NOW-Certificate-of-Occupancy.csv",
        description: "DOB NOW Certificates of Occupancy",
    },
    SocrataDataset {
        id: "p8u6-a6it",
        filename: "Digital-Tax-Map-Condominiums.csv",
        description: "Digital Tax Map Condominiums",
    },
    SocrataDataset {
        id: "ic3t-wcy2",
        filename: "DOB-Job-Application-Filings.csv",
        description: "DOB Job Application Filings",
    },
    SocrataDataset {
        id: "w9ak-ipjd",
        filename: "DOB-NOW-Build-Job-Application-Filings.csv",
        description: "DOB NOW Build Job Application Filings",
    },
    SocrataDataset {
        id: "ipu4-2q9a",
        filename: "DOB-Permit-Issuance.csv",
        description: "DOB Permit Issuance",
    },
    SocrataDataset {
        id: "rbx6-tga4",
        filename: "DOB-NOW-Build-Approved-Permits.csv",
        description: "DOB NOW Build Approved Permits",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_datasets_have_non_empty_ids() {
        for ds in NYC_OPEN_DATA_DATASETS {
            assert!(!ds.id.is_empty(), "Dataset {} has empty id", ds.filename);
            assert!(!ds.filename.is_empty());
            assert!(ds.filename.ends_with(".csv"));
        }
    }

    #[test]
    fn expected_datasets_are_registered() {
        let ids: Vec<&str> = NYC_OPEN_DATA_DATASETS.iter().map(|d| d.id).collect();
        assert!(ids.contains(&"pkdm-hqz6"), "missing certificates_of_occupancy");
        assert!(ids.contains(&"p8u6-a6it"), "missing condo_units");
        assert!(ids.contains(&"ic3t-wcy2"), "missing dob_job_applications");
        assert!(ids.contains(&"w9ak-ipjd"), "missing dob_now_job_applications");
        assert!(ids.contains(&"ipu4-2q9a"), "missing dob_permit_issuance");
        assert!(ids.contains(&"rbx6-tga4"), "missing dob_now_approved_permits");
    }
}
