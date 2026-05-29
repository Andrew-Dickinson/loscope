use clap::{Parser, Subcommand};
use loscope_preprocessing::nyc_tile_bounds::update_nyc_tiles_json;

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
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::NycTiles => update_nyc_tiles_json(),
    }
}
