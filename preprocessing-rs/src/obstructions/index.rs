use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use loscope::types::obstructions::ObstructionType;
use rayon::prelude::*;

/// Scan `{obstructions_dir}/{type}/*.json` files, build a tile→UUID index, and
/// write one `{out_dir}/{type}.json` per `ObstructionType`.
pub fn build_obstruction_index(obstructions_dir: &Path, out_dir: &Path) -> Result<()> {
    fs::create_dir_all(out_dir)?;

    // Collect (type_name, json_path) pairs up front so we can parallelise the reads.
    let mut json_paths: Vec<(String, std::path::PathBuf)> = Vec::new();
    for type_entry in fs::read_dir(obstructions_dir)? {
        let type_entry = type_entry?;
        if !type_entry.file_type()?.is_dir() {
            continue;
        }
        let type_name = type_entry.file_name().to_string_lossy().to_string();
        if ObstructionType::parse(&type_name).is_err() {
            continue;
        }
        for json_entry in fs::read_dir(type_entry.path())? {
            let json_entry = json_entry?;
            let path = json_entry.path();
            if path.extension().map(|e| e != "json").unwrap_or(true) {
                continue;
            }
            json_paths.push((type_name.clone(), path));
        }
    }

    let pb = ProgressBar::new(json_paths.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{bar:40} {pos}/{len} files | {per_sec} | eta: {eta}")
            .unwrap(),
    );

    // Parse files in parallel, collect (type_name, obstruction_id, tile_ids) triples.
    let entries: Vec<(String, String, Vec<String>)> = json_paths
        .par_iter()
        .filter_map(|(type_name, path)| {
            pb.inc(1);
            let text = fs::read_to_string(path).ok()?;
            let value: serde_json::Value = serde_json::from_str(&text).ok()?;
            let obstruction_id = value["obstruction_id"].as_str()?.to_string();
            let tile_ids = value["tile_ids"]
                .as_array()?
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>();
            Some((type_name.clone(), obstruction_id, tile_ids))
        })
        .collect();

    pb.finish_and_clear();

    // Merge sequentially — no locking needed since we're back on one thread.
    let mut index: HashMap<String, HashMap<String, Vec<String>>> = HashMap::new();
    for (type_name, obstruction_id, tile_ids) in entries {
        let type_map = index.entry(type_name).or_default();
        for tile_id in tile_ids {
            type_map.entry(tile_id).or_default().push(obstruction_id.clone());
        }
    }

    for (type_name, tile_map) in &index {
        let out_path = out_dir.join(format!("{type_name}.json"));
        let file = fs::File::create(&out_path)
            .with_context(|| format!("Failed to create {}", out_path.display()))?;
        serde_json::to_writer_pretty(file, tile_map)
            .with_context(|| format!("Failed to write {}", out_path.display()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn write_obs_json(dir: &Path, type_name: &str, uuid: &str, tile_ids: &[&str]) {
        let type_dir = dir.join(type_name);
        fs::create_dir_all(&type_dir).unwrap();
        let json = serde_json::json!({
            "obstruction_id": uuid,
            "obstruction_type": type_name,
            "tile_ids": tile_ids,
            "x_offset": 0, "y_offset": 0,
            "width": 1, "height": 1,
            "raster_file": format!("{uuid}.tif"),
            "attributes": {},
        });
        let mut f = fs::File::create(type_dir.join(format!("{uuid}.json"))).unwrap();
        f.write_all(json.to_string().as_bytes()).unwrap();
    }

    #[test]
    fn index_groups_uuids_by_tile() -> Result<()> {
        let obs_dir = tempdir()?;
        let out_dir = tempdir()?;

        let uuid1 = "00000000-0000-0000-0000-000000000001";
        let uuid2 = "00000000-0000-0000-0000-000000000002";

        write_obs_json(obs_dir.path(), "new_construction_footprints", uuid1, &["500300_23", "500300_24"]);
        write_obs_json(obs_dir.path(), "new_construction_footprints", uuid2, &["500300_23"]);

        build_obstruction_index(obs_dir.path(), out_dir.path())?;

        let index_path = out_dir.path().join("new_construction_footprints.json");
        assert!(index_path.exists());

        let content: HashMap<String, Vec<String>> =
            serde_json::from_str(&fs::read_to_string(index_path)?)?;

        let tile_23 = content.get("500300_23").expect("tile 500300_23 should be in index");
        assert_eq!(tile_23.len(), 2);
        assert!(tile_23.contains(&uuid1.to_string()));
        assert!(tile_23.contains(&uuid2.to_string()));

        let tile_24 = content.get("500300_24").expect("tile 500300_24 should be in index");
        assert_eq!(tile_24.len(), 1);
        assert_eq!(tile_24[0], uuid1);

        Ok(())
    }

    #[test]
    fn unknown_type_dirs_are_skipped() -> Result<()> {
        let obs_dir = tempdir()?;
        let out_dir = tempdir()?;

        // Create a dir with an unknown name — should be silently ignored.
        fs::create_dir(obs_dir.path().join("not_a_real_type"))?;

        build_obstruction_index(obs_dir.path(), out_dir.path())?;

        // Output dir should be empty.
        assert_eq!(fs::read_dir(out_dir.path())?.count(), 0);
        Ok(())
    }
}
