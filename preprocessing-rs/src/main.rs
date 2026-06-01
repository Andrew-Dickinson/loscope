use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use geo::Polygon;
use indicatif::{ProgressBar, ProgressStyle};
use loscope::building::heightmap::get_intersecting_tiles;
use loscope::types::coords::NYSCoords2;
use loscope::types::obstructions::{AttributeValue, ObstructionRaster, ObstructionType};
use loscope::types::tiles::LASTileId;
use loscope_preprocessing::nyc_tile_bounds::update_nyc_tiles_json;
use loscope_preprocessing::obstructions::{
    dem::max_ground_elevation_from_dem,
    index::build_obstruction_index,
    io::write_obstruction,
    model::ObstructionMetaOutput,
    rasterize::rasterize_polygon,
};
use ndarray::Array2;
use loscope_preprocessing::preprocess::{
    gap_fill::fill_gaps, io::write_tile, rasterize::build_height_grid, tiles::split_tiles,
};
use rayon::prelude::*;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "loscope-preprocessing", about = "NYC LOS preprocessing pipeline")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate nyc_tiles.json from the NYC boundary WKT file
    NycTiles,

    /// Rasterize LAS point clouds into 500×500 elevation tiles
    PreprocessTiles {
        /// Directory containing .las input files
        #[arg(long)]
        input: PathBuf,

        /// Directory to write .tif output tiles
        #[arg(long)]
        output: PathBuf,
    },

    /// Generate obstruction tif+json pairs by running a SQL query against nyc_dob.db
    BuildObstructions {
        /// Path to a .sql file whose results drive obstruction generation
        #[arg(long)]
        query: PathBuf,

        /// Path to the SQLite database (nyc_dob.db)
        #[arg(long)]
        db: PathBuf,

        /// Output directory for tif+json pairs (organised as {type}/{uuid}.*)
        #[arg(long)]
        output: PathBuf,

        /// Local DEM tile cache directory for ground elevation fallback
        #[arg(long, default_value = "data/dem_tiles")]
        dem_cache: PathBuf,
    },

    /// Build tile→UUID index files from obstruction JSON metadata
    BuildObstructionIndex {
        /// Directory containing per-type obstruction subdirs with JSON files
        #[arg(long)]
        obstructions: PathBuf,

        /// Directory to write {type}.json index files
        #[arg(long)]
        output: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::NycTiles => update_nyc_tiles_json(),
        Command::PreprocessTiles { input, output } => run_preprocess_tiles(&input, &output)?,
        Command::BuildObstructions { query, db, output, dem_cache } => {
            run_build_obstructions(&query, &db, &output, &dem_cache)?
        }
        Command::BuildObstructionIndex { obstructions, output } => {
            build_obstruction_index(&obstructions, &output)?
        }
    }

    Ok(())
}

fn run_preprocess_tiles(input: &PathBuf, output: &PathBuf) -> Result<()> {
    std::fs::create_dir_all(output)?;

    let las_files: Vec<PathBuf> = std::fs::read_dir(input)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "las").unwrap_or(false))
        .collect();

    if las_files.is_empty() {
        eprintln!("No .las files found in {}", input.display());
        return Ok(());
    }

    println!(
        "Found {} .las file(s) → writing tiles to {}",
        las_files.len(),
        output.display()
    );

    let pb = ProgressBar::new(las_files.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{bar:40} {pos}/{len} files | {per_sec} | eta: {eta}")
            .unwrap(),
    );

    let results: Vec<anyhow::Result<usize>> = las_files
        .par_iter()
        .map(|las_path| {
            let stem = las_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let las_id = LASTileId::parse(stem)
                .map_err(|e| anyhow::anyhow!("Invalid LAS filename {:?}: {:?}", stem, e))?;

            let (height_grid, count_grid) =
                build_height_grid(las_path.to_str().unwrap(), las_id)?;
            let filled = fill_gaps(&height_grid, &count_grid);
            let tiles = split_tiles(&filled, las_id);
            let n_tiles = tiles.len();
            for tile in &tiles {
                write_tile(tile, output)?;
            }
            pb.inc(1);
            Ok(n_tiles)
        })
        .collect();

    pb.finish_and_clear();

    let mut total_tiles = 0usize;
    let mut errors = 0usize;
    for res in results {
        match res {
            Ok(n) => total_tiles += n,
            Err(e) => {
                eprintln!("Error: {e}");
                errors += 1;
            }
        }
    }

    println!(
        "Done: {total_tiles} tiles written from {} files ({errors} errors).",
        las_files.len()
    );
    Ok(())
}

fn run_build_obstructions(
    query_path: &PathBuf,
    db_path: &PathBuf,
    out_dir: &PathBuf,
    dem_cache: &PathBuf,
) -> Result<()> {
    use rusqlite::{Connection, OpenFlags};
    use wkt::TryFromWkt;

    std::fs::create_dir_all(out_dir)?;

    let sql = std::fs::read_to_string(query_path)
        .map_err(|e| anyhow::anyhow!("Cannot read query file {}: {e}", query_path.display()))?;

    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.execute_batch("PRAGMA cache_size=-65536; PRAGMA temp_store=MEMORY;")?;

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;

    let mut written = 0usize;
    let mut skipped = 0usize;

    while let Some(row) = rows.next()? {
        let geom_str: Option<String> = row.get(0)?; // output_geometry
        let ground_elevation: Option<f64> = row.get(1)?; // may be NULL
        let height_roof: Option<f64> = row.get(2)?;
        let type_str: Option<String> = row.get(3)?;
        let props_str: Option<String> = row.get(4)?;

        let (geom_str, height_roof, type_str, props_str) =
            match (geom_str, height_roof, type_str, props_str) {
                (Some(g), Some(h), Some(t), Some(p)) => (g, h, t, p),
                _ => { skipped += 1; continue; }
            };

        if geom_str.is_empty() { skipped += 1; continue; }

        let obs_type = match ObstructionType::parse(&type_str) {
            Ok(t) => t,
            Err(_) => {
                eprintln!("Unknown obstruction type: {type_str}");
                skipped += 1;
                continue;
            }
        };

        let poly: Polygon<f64> = match Polygon::try_from_wkt_str(&geom_str) {
            Ok(p) => p,
            Err(_) => { skipped += 1; continue; }
        };

        let ground_elev = match ground_elevation {
            Some(e) => e,
            None => match max_ground_elevation_from_dem(&poly, dem_cache) {
                Some(e) => e,
                None => { skipped += 1; continue; }
            },
        };

        let total_height_inches = ((ground_elev + height_roof) * 12.0).round() as u16;
        let tile_ids = match get_intersecting_tiles(&poly) {
            Ok((tiles, _)) => tiles,
            Err(_) => { skipped += 1; continue; }
        };
        let (x_sw, y_sw, w, h, flat_raster) = rasterize_polygon(&poly, total_height_inches);

        if flat_raster.iter().all(|&v| v == 0) { skipped += 1; continue; }

        let uuid = Uuid::new_v4();
        let mut attributes: HashMap<String, AttributeValue> =
            serde_json::from_str(&props_str).unwrap_or_default();
        attributes.insert(
            "ground_elevation".to_string(),
            AttributeValue::Number(serde_json::Number::from_f64(ground_elev).unwrap_or(0.into())),
        );

        let raster = ObstructionRaster::new(
            Array2::from_shape_vec((w as usize, h as usize), flat_raster).unwrap(),
        );
        let meta = ObstructionMetaOutput {
            obstruction_id: uuid,
            obstruction_type: obs_type,
            attributes,
            tile_ids,
            offset_nys: NYSCoords2::new(x_sw as f64, y_sw as f64),
            width: w as usize,
            height: h as usize,
            raster_file: format!("{uuid}.tif"),
        };

        write_obstruction(&meta, &raster, out_dir)?;
        written += 1;
    }

    if skipped > 0 {
        eprintln!("Skipped {skipped} rows with missing/invalid data");
    }
    println!("Done. {written} obstruction(s) written.");
    Ok(())
}
