use std::collections::HashSet;
use std::fs;
use std::fs::File;
use geo::{coord, BoundingRect, Contains, MapCoords, MultiPolygon};
use indicatif::{ProgressBar, ProgressStyle};
use loscope::types::errors::AssetErr;
use wkt::TryFromWkt;
use loscope::types::coords::NYSCoords2;
use loscope::types::tiles::{TileId, SUBGRID_TILE_SIDE_LENGTH_USFT};
use rayon::prelude::*;

pub struct NycTileGenerator {
    poly: MultiPolygon<f64>
}

impl NycTileGenerator {

    pub fn from_wkt_file(file_name: &str) -> Self {
        let wkt_string = fs::read_to_string(file_name).unwrap();

        NycTileGenerator {
            poly: MultiPolygon::try_from_wkt_str(&wkt_string)
                .map_err(|err| AssetErr::AssetContentError(format!(
                    "Invalid WKT string found in {file_name}: {err}"
                ))).unwrap()
        }
    }


    pub fn candidate_tiles_iter(&self) -> impl Iterator<Item=TileId> {
        let bounds = self.poly.bounding_rect().unwrap();

        let ne_corner = bounds.max();
        let sw_corner = bounds.min();

        let tiles_tall = ((ne_corner.y - sw_corner.y) / SUBGRID_TILE_SIDE_LENGTH_USFT as f64).ceil() as usize;
        let tiles_wide = ((ne_corner.x - sw_corner.x) / SUBGRID_TILE_SIDE_LENGTH_USFT as f64).ceil() as usize;

        let sw_tile = TileId::from_contained_point(
            &NYSCoords2::from(<(_, _)>::from(sw_corner)
            ));

        (0..=tiles_wide).flat_map(move |i| {
            (0..=tiles_tall).map(
                move |j| {
                    TileId::from_adjacent_tile(&sw_tile, (i as isize, j as isize))
                }
            )
        })
    }

    pub fn tile_contained(&self, tile_id: &TileId) -> bool {
        self.poly.contains(&tile_id.get_bounds()
            .map_coords(|u| coord! {x: u.x as f64, y: u.y as f64}))
    }

    pub fn contained_tiles_iter(&self) -> impl Iterator<Item=TileId> {
        self.candidate_tiles_iter()
            .filter(|tile_id| self.tile_contained(tile_id))
    }
}
const NYC_BOUNDS_WKT_FILE: &str = "../bundled_geo_data/nyc_bounds_wkt.txt";
const TILE_IDS_OUTPUT_FILE: &str = "../backend-rs/static_resources/nyc_tiles.json";

pub fn update_nyc_tiles_json() {
    let bounds = NycTileGenerator::from_wkt_file(NYC_BOUNDS_WKT_FILE);

    let candidates: HashSet<_> = bounds.candidate_tiles_iter().collect();

    let pb = ProgressBar::new(candidates.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{bar:40} {pos}/{len} candidates | {per_sec} | eta: {eta}")
            .unwrap(),
    );

    let tiles: HashSet<_> = candidates
        .into_par_iter()
        .filter(|tile| {
            let contained = bounds.tile_contained(tile);
            pb.inc(1);
            if pb.position().is_multiple_of(20) {
                pb.tick();
            }
            contained
        })
        .collect();

    pb.finish_and_clear();
    println!(
        "Done: {} tiles in {:.2}s",
        tiles.len(),
        pb.elapsed().as_secs_f64()
    );

    let mut serializable: Vec<_> = tiles
        .iter()
        .map(|t| serde_json::json!({"id": t.to_string(), "enc": t.as_packed_u64()}))
        .collect();
    serializable.sort_by_key(|v| v["enc"].as_u64());

    let output_file = File::create(TILE_IDS_OUTPUT_FILE).unwrap();
    serde_json::to_writer(output_file, &serializable).unwrap()
}
