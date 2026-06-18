use std::path::Path;

use anyhow::Result;

use super::{arcgis, socrata};

const BUILDING_FOOTPRINTS_URL: &str =
    "https://services6.arcgis.com/yG5s3afENB5iO9fj/arcgis/rest/services/BUILDING_view/FeatureServer/0";
const TAX_LOTS_URL: &str =
    "https://services6.arcgis.com/yG5s3afENB5iO9fj/ArcGIS/rest/services/DTM_ETL_DAILY_view/FeatureServer/0";

/// Download all input datasets required by `build-database`.
///
/// Geographic datasets (building footprints, tax lots) are fetched from ArcGIS;
/// DOB tabular datasets are fetched from NYC Open Data (Socrata). All files are
/// written to `out_dir`.
pub fn download_all(out_dir: &Path, chunk_size: usize, creds: &socrata::Credentials) -> Result<()> {
    std::fs::create_dir_all(out_dir)?;

    println!("--- Downloading building footprints from ArcGIS ---");
    arcgis::download(
        BUILDING_FOOTPRINTS_URL,
        &out_dir.join("building-footprints.csv"),
        "1=1",
        chunk_size,
    )?;

    println!("--- Downloading tax lots from ArcGIS ---");
    arcgis::download(
        TAX_LOTS_URL,
        &out_dir.join("tax-lots.csv"),
        "1=1",
        chunk_size,
    )?;

    println!("--- Downloading NYC Open Data CSVs ---");
    for ds in socrata::NYC_OPEN_DATA_DATASETS {
        println!("  Downloading {} …", ds.description);
        socrata::download_bulk(ds.id, &out_dir.join(ds.filename), creds)?;
    }

    println!("All downloads complete.");
    Ok(())
}
