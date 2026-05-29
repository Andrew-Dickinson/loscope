use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::blocking::Client;

const SOCRATA_BASE: &str = "https://data.cityofnewyork.us/resource";

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
