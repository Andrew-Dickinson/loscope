use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use loscope::types::tiles::LASTileId;
use loscope_preprocessing::nyc_tile_bounds::update_nyc_tiles_json;
use loscope_preprocessing::preprocess::{gap_fill::fill_gaps, io::write_tile, rasterize::build_height_grid, tiles::split_tiles};
use rayon::prelude::*;

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
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::NycTiles => update_nyc_tiles_json(),
        Command::PreprocessTiles { input, output } => run_preprocess_tiles(&input, &output)?,
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
