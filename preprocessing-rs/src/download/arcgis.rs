use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::blocking::Client;
use serde_json::Value;

const OUT_SR: u32 = 6539; // NAD83(2011) / NY Long Island (US survey feet)

/// Build a reqwest blocking client that accepts invalid TLS certs (matching
/// Python's InsecureRequestWarning suppression for the NYC ArcGIS portal).
pub fn build_client() -> Result<Client> {
    Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .context("Failed to build HTTP client")
}

/// Convert ArcGIS esri JSON polygon geometry to WKT.
///
/// The `rings` field is an array of coordinate rings. The first ring is the
/// exterior; subsequent rings are holes. We produce a POLYGON or MULTIPOLYGON.
pub fn rings_to_wkt(geometry: &Value) -> String {
    let rings = match geometry.get("rings").and_then(|r| r.as_array()) {
        Some(r) => r,
        None => return String::new(),
    };

    if rings.is_empty() {
        return String::new();
    }

    let format_ring = |ring: &Value| -> String {
        let coords = ring
            .as_array()
            .map(|pts| {
                pts.iter()
                    .filter_map(|pt| {
                        let arr = pt.as_array()?;
                        let x = arr.first()?.as_f64()?;
                        let y = arr.get(1)?.as_f64()?;
                        Some(format!("{x} {y}"))
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        format!("({coords})")
    };

    if rings.len() == 1 {
        format!("POLYGON ({})", format_ring(&rings[0]))
    } else {
        // ArcGIS uses multiple rings: exterior + holes, all in the same Polygon.
        let ring_strs: Vec<String> = rings.iter().map(format_ring).collect();
        format!("POLYGON ({})", ring_strs.join(", "))
    }
}

/// Download all features from an ArcGIS FeatureServer layer to a CSV file.
///
/// Geometry is requested in EPSG:6539 (NYS State Plane Long Island, US survey feet)
/// and stored as WKT in the `the_geom` column. Attribute keys are uppercased to
/// match the Python implementation.
pub fn download(
    url: &str,
    out_path: &Path,
    where_clause: &str,
    chunk_size: usize,
) -> Result<()> {
    let client = build_client()?;

    // Get total feature count.
    let count_url = format!(
        "{url}/query?where={}&returnCountOnly=true&f=json",
        urlencoding(where_clause)
    );
    let count_resp: Value = client
        .get(&count_url)
        .send()
        .context("Failed to fetch feature count")?
        .json()
        .context("Failed to parse feature count response")?;
    let total = count_resp["count"].as_u64().unwrap_or(0) as usize;

    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{bar:40} {pos}/{len} features | {per_sec} | eta: {eta}")
            .unwrap(),
    );

    let file = std::fs::File::create(out_path)
        .with_context(|| format!("Cannot create {}", out_path.display()))?;
    let mut writer: Option<csv::Writer<BufWriter<std::fs::File>>> = None;

    let mut offset = 0usize;
    loop {
        let page_url = format!(
            "{url}/query?where={}&outFields=*&outSR={OUT_SR}&returnGeometry=true\
             &resultOffset={offset}&resultRecordCount={chunk_size}&f=json",
            urlencoding(where_clause)
        );
        let resp: Value = client
            .get(&page_url)
            .send()
            .with_context(|| format!("Failed to fetch features at offset {offset}"))?
            .json()
            .context("Failed to parse feature page")?;

        let features = match resp["features"].as_array() {
            Some(f) if !f.is_empty() => f,
            _ => break,
        };

        for feat in features {
            let geom_wkt = feat
                .get("geometry")
                .map(rings_to_wkt)
                .unwrap_or_default();

            let attrs = feat["attributes"].as_object();
            let mut row: indexmap::IndexMap<String, String> = indexmap::IndexMap::new();
            row.insert("the_geom".to_string(), geom_wkt);

            if let Some(attrs) = attrs {
                for (k, v) in attrs {
                    let key = k.to_uppercase();
                    let val = match v {
                        Value::Null => String::new(),
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    row.insert(key, val);
                }
            }

            if writer.is_none() {
                let mut w = csv::Writer::from_writer(BufWriter::new(file.try_clone()?));
                w.write_record(row.keys())?;
                writer = Some(w);
            }
            writer.as_mut().unwrap().write_record(row.values())?;
            pb.inc(1);
        }

        let fetched = features.len();
        offset += fetched;
        if fetched < chunk_size {
            break;
        }
    }

    if let Some(mut w) = writer {
        w.flush()?;
    }
    pb.finish_and_clear();
    println!("Wrote {} features to {}", offset, out_path.display());
    Ok(())
}

fn urlencoding(s: &str) -> String {
    s.replace(' ', "%20")
        .replace('=', "%3D")
        .replace('>', "%3E")
        .replace('<', "%3C")
        .replace('\'', "%27")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn single_ring_polygon_to_wkt() {
        let geom = json!({
            "rings": [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]]]
        });
        let wkt = rings_to_wkt(&geom);
        assert_eq!(wkt, "POLYGON ((0 0, 1 0, 1 1, 0 1, 0 0))");
    }

    #[test]
    fn multi_ring_uses_polygon_with_holes() {
        let geom = json!({
            "rings": [
                [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]],
                [[2.0, 2.0], [4.0, 2.0], [4.0, 4.0], [2.0, 4.0], [2.0, 2.0]]
            ]
        });
        let wkt = rings_to_wkt(&geom);
        assert!(wkt.starts_with("POLYGON ("));
        assert!(wkt.contains("0 0, 10 0"));
        assert!(wkt.contains("2 2, 4 2"));
    }

    #[test]
    fn null_geometry_gives_empty_string() {
        let geom = json!(null);
        assert_eq!(rings_to_wkt(&geom), "");
    }

    #[test]
    fn empty_rings_gives_empty_string() {
        let geom = json!({ "rings": [] });
        assert_eq!(rings_to_wkt(&geom), "");
    }
}
