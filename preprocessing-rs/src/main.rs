use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use geo::Polygon;
use indicatif::{ProgressBar, ProgressStyle};
use loscope::building::heightmap::get_intersecting_tiles;
use loscope::types::coords::NYSCoords2;
use loscope::types::obstructions::{AttributeValue, ObstructionRaster, ObstructionType};
use loscope::types::tiles::LASTileId;
use loscope_preprocessing::nyc_tile_bounds::update_nyc_tiles_json;
use loscope_preprocessing::database::{ingest, schema};
use loscope_preprocessing::dem::preprocess::split_dem;
use loscope_preprocessing::footprint_wkt::export::export_footprint_wkt;
use loscope_preprocessing::download::{arcgis, city_data, planimetrics, socrata};
use loscope_preprocessing::obstructions::{
    dem::max_ground_elevation_from_dem,
    index::build_obstruction_index,
    io::write_obstruction,
    model::ObstructionMetaOutput,
    rasterize::rasterize_polygon,
};
use ndarray::Array2;
use loscope_preprocessing::preprocess::{
    classify::{
        build_class_grid, filter_polys_for_tile, load_building_footprints,
        load_osm_hydro_structures, load_osm_land_polys, load_planimetrics_csv, tile_bbox,
        PixelClass,
    },
    gap_fill::fill_gaps,
    io::{write_class_tile, write_tile},
    rasterize::build_height_grid,
    tiles::split_tiles_with_class,
};
use rayon::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
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

    /// Rasterize LAS point clouds into 500×500 elevation tiles and classification tiles
    PreprocessTiles {
        /// Directory containing .las input files
        #[arg(long)]
        input: PathBuf,

        /// Directory to write .tif output tiles (classification tiles go in output/classification/)
        #[arg(long)]
        output: PathBuf,

        /// Path to the nyc_dob.db SQLite database (output of build-database)
        #[arg(long)]
        features_db: PathBuf,

        /// Path to the OSM land-polygons shapefile (WGS84 / EPSG 4326)
        #[arg(long)]
        osm_land_polys: PathBuf,

        /// Path to the OSM hydro-structures GeoJSON (WGS84 / EPSG 4326)
        #[arg(long)]
        osm_hydro_structures: PathBuf,

        /// Path to the planimetrics misc-structure-poly CSV (the_geom in EPSG 6539)
        #[arg(long)]
        planimetrics_misc_structures: PathBuf,

        /// Path to the planimetrics hydro-structure CSV (the_geom in EPSG 6539)
        #[arg(long)]
        planimetrics_hydro_structures: PathBuf,

        /// Set elevation to 0 for pixels classified as water
        #[arg(long, default_value_t = false)]
        zero_water_elevation: bool,
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

    /// Download a single public ArcGIS FeatureServer layer to CSV
    DownloadArcgis {
        /// ArcGIS FeatureServer layer URL
        url: String,

        /// Output CSV path
        #[arg(long)]
        output: PathBuf,

        /// Server-side WHERE clause (default: 1=1)
        #[arg(long, default_value = "1=1")]
        r#where: String,

        /// Features per page
        #[arg(long, default_value_t = 1000)]
        chunk: usize,
    },

    /// Download NYC Open Data (Socrata) DOB tabular CSVs
    DownloadOpendata {
        /// Output directory
        #[arg(long)]
        output: PathBuf,

        /// Rows per page
        #[arg(long, default_value_t = 50000)]
        chunk: usize,
    },

    /// Download all input datasets (ArcGIS + NYC Open Data)
    DownloadCityData {
        /// Output directory
        #[arg(long)]
        output: PathBuf,

        /// Features/rows per page
        #[arg(long, default_value_t = 1000)]
        chunk: usize,
    },

    /// Build the nyc_dob.db SQLite database from downloaded CSV files
    BuildDatabase {
        /// Output database path
        #[arg(long)]
        output: PathBuf,

        #[arg(long)] footprints: Option<PathBuf>,
        #[arg(long)] tax_lots: Option<PathBuf>,
        #[arg(long)] dob_jobs: Option<PathBuf>,
        #[arg(long)] dob_now_jobs: Option<PathBuf>,
        #[arg(long)] cos: Option<PathBuf>,
        #[arg(long)] permits: Option<PathBuf>,
        #[arg(long)] now_permits: Option<PathBuf>,
        #[arg(long)] condos: Option<PathBuf>,
    },

    /// Export per-BIN footprint WKT files from the SQLite database
    BuildFootprintWkt {
        /// Path to nyc_dob.db
        #[arg(long)]
        db: PathBuf,

        /// Output directory for {bin}.wkt files
        #[arg(long)]
        output: PathBuf,
    },

    /// Download 2022 NYC Planimetrics layers from ArcGIS (all layers, or one by slug)
    DownloadPlanimetrics {
        /// Output directory for CSV files
        #[arg(long)]
        output: PathBuf,

        /// Download only this layer (slug, e.g. "hydrography"); omit to download all 26 layers
        #[arg(long)]
        layer: Option<String>,

        /// Features per page
        #[arg(long, default_value_t = 1000)]
        chunk: usize,
    },

    /// Split a citywide DEM GeoTIFF into canonical 500-usft elevation tiles
    PreprocessDem {
        /// Path to the input DEM GeoTIFF (EPSG:6539+6360, 1 usft/pixel, heights in usft)
        dem_tif: PathBuf,

        /// Output directory for tile .tif and .json files
        #[arg(long, default_value = "data/dem_tiles")]
        output: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::NycTiles => update_nyc_tiles_json(),
        Command::PreprocessTiles {
            input, output, features_db,
            osm_land_polys, osm_hydro_structures,
            planimetrics_misc_structures, planimetrics_hydro_structures,
            zero_water_elevation,
        } => run_preprocess_tiles(
            &input, &output, &features_db,
            &osm_land_polys, &osm_hydro_structures,
            &planimetrics_misc_structures, &planimetrics_hydro_structures,
            zero_water_elevation,
        )?,
        Command::BuildObstructions { query, db, output, dem_cache } => {
            run_build_obstructions(&query, &db, &output, &dem_cache)?
        }
        Command::BuildObstructionIndex { obstructions, output } => {
            build_obstruction_index(&obstructions, &output)?
        }
        Command::DownloadArcgis { url, output, r#where, chunk } => {
            arcgis::download(&url, &output, &r#where, chunk)?
        }
        Command::DownloadOpendata { output, chunk: _ } => {
            std::fs::create_dir_all(&output)?;
            for ds in socrata::NYC_OPEN_DATA_DATASETS {
                println!("Downloading {} …", ds.description);
                socrata::download_bulk(ds.id, &output.join(ds.filename))?;
            }
        }
        Command::DownloadCityData { output, chunk } => {
            city_data::download_all(&output, chunk)?
        }
        Command::BuildDatabase {
            output, footprints, tax_lots, dob_jobs, dob_now_jobs, cos, permits, now_permits, condos,
        } => run_build_database(
            &output, footprints, tax_lots, dob_jobs, dob_now_jobs, cos, permits, now_permits, condos,
        )?,
        Command::BuildFootprintWkt { db, output } => {
            let count = export_footprint_wkt(&db, &output)?;
            println!("Wrote {count} .wkt files to {}", output.display());
        }
        Command::DownloadPlanimetrics { output, layer, chunk } => {
            match layer {
                Some(slug) => planimetrics::download_one(&slug, &output, chunk)?,
                None => planimetrics::download_all(&output, chunk)?,
            }
        }
        Command::PreprocessDem { dem_tif, output } => {
            split_dem(&dem_tif, &output)?;
        }
    }

    Ok(())
}

fn run_preprocess_tiles(
    input: &PathBuf,
    output: &PathBuf,
    features_db: &PathBuf,
    osm_land_polys: &PathBuf,
    osm_hydro_structures: &PathBuf,
    planimetrics_misc_structures: &PathBuf,
    planimetrics_hydro_structures: &PathBuf,
    zero_water_elevation: bool,
) -> Result<()> {
    std::fs::create_dir_all(output)?;

    println!("Loading building footprints …");
    let building_footprints = load_building_footprints(features_db)
        .with_context(|| format!("Failed to load building footprints from {}", features_db.display()))?;
    println!("  → {} footprints", building_footprints.len());

    println!("Loading planimetrics misc structures …");
    let misc_structures = load_planimetrics_csv(planimetrics_misc_structures)
        .with_context(|| format!("Failed to load misc structures from {}", planimetrics_misc_structures.display()))?;
    println!("  → {} misc structures", misc_structures.len());

    println!("Loading planimetrics hydro structures …");
    let planimetrics_hydro = load_planimetrics_csv(planimetrics_hydro_structures)
        .with_context(|| format!("Failed to load hydro structures from {}", planimetrics_hydro_structures.display()))?;
    println!("  → {} planimetrics hydro structures", planimetrics_hydro.len());

    println!("Loading OSM land polygons …");
    let land_polys = load_osm_land_polys(osm_land_polys)
        .with_context(|| format!("Failed to load OSM land polygons from {}", osm_land_polys.display()))?;
    println!("  → {} land polygons", land_polys.len());

    println!("Loading OSM hydro structures …");
    let osm_hydro = load_osm_hydro_structures(osm_hydro_structures)
        .with_context(|| format!("Failed to load OSM hydro structures from {}", osm_hydro_structures.display()))?;
    println!("  → {} OSM hydro structures", osm_hydro.len());

    // Combine both hydro sources into one list for tile filtering
    let mut all_hydro = osm_hydro;
    all_hydro.extend(planimetrics_hydro);

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

    // Arc-wrap shared read-only data for rayon parallelism
    let building_footprints = std::sync::Arc::new(building_footprints);
    let misc_structures = std::sync::Arc::new(misc_structures);
    let all_hydro = std::sync::Arc::new(all_hydro);
    let land_polys = std::sync::Arc::new(land_polys);

    let results: Vec<anyhow::Result<usize>> = las_files
        .par_iter()
        .map(|las_path| {
            let stem = las_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let las_id = LASTileId::parse(stem)
                .map_err(|e| anyhow::anyhow!("Invalid LAS filename {:?}: {:?}", stem, e))?;

            let (height_grid, count_grid, veg_grid) =
                build_height_grid(las_path.to_str().unwrap(), las_id)?;

            let tbbox = tile_bbox(las_id);
            let tile_buildings = filter_polys_for_tile(&building_footprints, tbbox);
            let tile_misc = filter_polys_for_tile(&misc_structures, tbbox);
            let tile_hydro = filter_polys_for_tile(&all_hydro, tbbox);
            let tile_land = filter_polys_for_tile(&land_polys, tbbox);

            let class_grid = build_class_grid(
                &height_grid, &veg_grid,
                &tile_buildings, &tile_misc,
                &tile_hydro, &tile_land,
                las_id,
            );

            let mut filled = fill_gaps(&height_grid, &count_grid);

            if zero_water_elevation {
                for (idx, &cls) in class_grid.iter().enumerate() {
                    if cls == PixelClass::Water as u8 {
                        filled[idx] = 0;
                    }
                }
            }

            let tile_pairs = split_tiles_with_class(&filled, &class_grid, las_id);
            let n_tiles = tile_pairs.len();
            for (tile, class_sub) in &tile_pairs {
                write_tile(tile, output)?;
                write_class_tile(tile.id(), class_sub, output)?;
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

#[allow(clippy::too_many_arguments)]
fn run_build_database(
    db_path: &Path,
    footprints: Option<PathBuf>,
    tax_lots: Option<PathBuf>,
    dob_jobs: Option<PathBuf>,
    dob_now_jobs: Option<PathBuf>,
    cos: Option<PathBuf>,
    permits: Option<PathBuf>,
    now_permits: Option<PathBuf>,
    condos: Option<PathBuf>,
) -> Result<()> {
    use rusqlite::Connection;

    let conn = Connection::open(db_path)
        .map_err(|e| anyhow::anyhow!("Cannot open/create {}: {e}", db_path.display()))?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch(schema::CREATE_TABLES)?;

    macro_rules! ingest_if {
        ($opt:expr, $fn:ident, $label:literal) => {
            if let Some(path) = $opt {
                println!("Ingesting {} from {} …", $label, path.display());
                let n = ingest::$fn(&conn, &path)?;
                println!("  → {n} rows");
            }
        };
    }

    ingest_if!(footprints, ingest_building_footprints, "building_footprints");
    ingest_if!(tax_lots, ingest_tax_lots, "tax_lots");
    ingest_if!(dob_jobs, ingest_dob_job_applications, "dob_job_applications");
    ingest_if!(dob_now_jobs, ingest_dob_now_job_applications, "dob_now_job_applications");
    ingest_if!(cos, ingest_certificates_of_occupancy, "certificates_of_occupancy");
    ingest_if!(permits, ingest_dob_permit_issuance, "dob_permit_issuance");
    ingest_if!(now_permits, ingest_dob_now_approved_permits, "dob_now_approved_permits");
    ingest_if!(condos, ingest_condo_units, "condo_units");

    println!("Creating indexes …");
    conn.execute_batch(schema::CREATE_INDEXES)?;
    println!("Done. Database written to {}", db_path.display());
    Ok(())
}

fn run_build_obstructions(
    query_path: &Path,
    db_path: &Path,
    out_dir: &Path,
    dem_cache: &Path,
) -> Result<()> {
    use rusqlite::{Connection, OpenFlags};
    use wkt::TryFromWkt;

    std::fs::create_dir_all(out_dir)?;

    let sql = std::fs::read_to_string(query_path)
        .map_err(|e| anyhow::anyhow!("Cannot read query file {}: {e}", query_path.display()))?;

    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.execute_batch("PRAGMA cache_size=-65536; PRAGMA temp_store=MEMORY;")?;

    struct RawRow {
        geom_str: Option<String>,
        ground_elevation: Option<f64>,
        height_roof: Option<f64>,
        type_str: Option<String>,
        props_str: Option<String>,
    }

    let mut stmt = conn.prepare(&sql)?;
    let raw_rows: Vec<RawRow> = stmt
        .query_map([], |row| {
            Ok(RawRow {
                geom_str: row.get("output_geometry")?,
                ground_elevation: row.get("ground_elevation")?,
                height_roof: row.get("height_roof")?,
                type_str: row.get("type")?,
                props_str: row.get("props")?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let pb = ProgressBar::new(raw_rows.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{bar:40} {pos}/{len} rows | {per_sec} | eta: {eta}")
            .unwrap(),
    );

    let written = AtomicUsize::new(0);
    let skipped = AtomicUsize::new(0);

    raw_rows.par_iter().for_each(|raw| {
        pb.inc(1);

        let result: anyhow::Result<bool> = (|| {
            let (geom_str, height_roof, type_str, props_str) = match (
                raw.geom_str.clone(),
                raw.height_roof,
                raw.type_str.clone(),
                raw.props_str.clone(),
            ) {
                (Some(g), Some(h), Some(t), Some(p)) => (g, h, t, p),
                _ => return Ok(false),
            };

            if geom_str.is_empty() {
                return Ok(false);
            }

            let obs_type = match ObstructionType::parse(&type_str) {
                Ok(t) => t,
                Err(_) => {
                    eprintln!("Unknown obstruction type: {type_str}");
                    return Ok(false);
                }
            };

            let poly: Polygon<f64> = match Polygon::try_from_wkt_str(&geom_str) {
                Ok(p) => p,
                Err(_) => return Ok(false),
            };

            let ground_elev = match raw.ground_elevation {
                Some(e) => e,
                None => match max_ground_elevation_from_dem(&poly, dem_cache) {
                    Some(e) => e,
                    None => return Ok(false),
                },
            };

            let total_height_inches = ((ground_elev + height_roof) * 12.0).round() as u16;
            let tile_ids = match get_intersecting_tiles(&poly) {
                Ok((tiles, _)) => tiles,
                Err(_) => return Ok(false),
            };
            let (x_sw, y_sw, w, h, flat_raster) = rasterize_polygon(&poly, total_height_inches);

            if flat_raster.iter().all(|&v| v == 0) {
                return Ok(false);
            }

            let uuid = Uuid::new_v4();
            let mut attributes: HashMap<String, AttributeValue> =
                serde_json::from_str(&props_str).unwrap_or_default();
            attributes.insert(
                "ground_elevation".to_string(),
                AttributeValue::Number(
                    serde_json::Number::from_f64(ground_elev).unwrap_or(0.into()),
                ),
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
            Ok(true)
        })();

        match result {
            Ok(true) => { written.fetch_add(1, Ordering::Relaxed); }
            Ok(false) => { skipped.fetch_add(1, Ordering::Relaxed); }
            Err(e) => {
                eprintln!("Error: {e}");
                skipped.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    pb.finish_and_clear();
    let written = written.load(Ordering::Relaxed);
    let skipped = skipped.load(Ordering::Relaxed);

    if skipped > 0 {
        eprintln!("Skipped {skipped} rows with missing/invalid data");
    }
    println!("Done. {written} obstruction(s) written.");
    Ok(())
}
