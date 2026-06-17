use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use loscope::types::coords::NYSCoords2;
use loscope::types::tiles::{TileId, SUBGRID_TILE_SIDE_LENGTH_USFT};
use tiff::decoder::{Decoder, DecodingResult, Limits};
use tiff::encoder::{TiffEncoder, colortype};

// NW (top-left) corner of the citywide 2010 DEM, in NYS State Plane easting/northing.
// Every pixel's NW corner is at (DEM_ORIGIN_E + col, DEM_ORIGIN_N - row).
const DEM_ORIGIN_E: i64 = 910_720;
const DEM_ORIGIN_N: i64 = 275_160;

const TILE_SIDE: usize = SUBGRID_TILE_SIDE_LENGTH_USFT as usize;

/// Extract a 500×500 uint16 raster (inches) for the canonical tile at (e_sw, n_sw).
///
/// Input DEM is in [row, col] layout (row 0 = northernmost). Heights are in US survey
/// feet; output values are inches (feet × 12, clamped to u16). Returns None if the
/// tile has no overlap with the DEM or if the result is all-zero.
pub fn extract_dem_tile(
    dem: &[i32],
    dem_h: usize,
    dem_w: usize,
    e_sw: i64,
    n_sw: i64,
) -> Option<Vec<u16>> {
    let col_start = e_sw - DEM_ORIGIN_E;
    let col_end = col_start + TILE_SIDE as i64;
    let row_end = DEM_ORIGIN_N - n_sw;
    let row_start = row_end - TILE_SIDE as i64;

    // Skip tiles fully outside the DEM.
    if col_end <= 0
        || col_start >= dem_w as i64
        || row_end <= 0
        || row_start >= dem_h as i64
    {
        return None;
    }

    let ac_rs = row_start.max(0) as usize;
    let ac_re = row_end.min(dem_h as i64) as usize;
    let ac_cs = col_start.max(0) as usize;
    let ac_ce = col_end.min(dem_w as i64) as usize;

    // Destination offsets within the 500×500 output tile.
    let dst_e = (ac_cs as i64 - col_start) as usize;
    let dst_n = (row_end - ac_re as i64) as usize;

    let mut out = vec![0.0f64; TILE_SIDE * TILE_SIDE];

    for (sub_row, row) in (ac_rs..ac_re).enumerate() {
        for (sub_col, col) in (ac_cs..ac_ce).enumerate() {
            let h_ft = dem[row * dem_w + col] as f64;
            let x = dst_e + sub_col;
            let y = dst_n + (ac_re - ac_rs - 1 - sub_row);
            if x < TILE_SIDE && y < TILE_SIDE {
                out[x * TILE_SIDE + y] = h_ft;
            }
        }
    }

    let result: Vec<u16> = out
        .iter()
        .map(|&h| (h * 12.0).round().clamp(0.0, 65535.0) as u16)
        .collect();

    if result.iter().all(|&v| v == 0) {
        return None;
    }

    Some(result)
}

/// Convert a completed f64 tile buffer to u16 inches and write it as Gray16 TIFF.
/// Returns the tile ID string, or None if the tile is all-zero.
fn flush_tile(buf: Vec<f64>, e_sw: i64, n_sw: i64, out_dir: &Path) -> Result<Option<String>> {
    let result: Vec<u16> = buf
        .iter()
        .map(|&h| (h * 12.0).round().clamp(0.0, 65535.0) as u16)
        .collect();

    if result.iter().all(|&v| v == 0) {
        return Ok(None);
    }

    let center = NYSCoords2::new(e_sw as f64 + 0.5, n_sw as f64 + 0.5);
    let tile_id = TileId::from_contained_point(&center);
    let tile_id_str = tile_id.to_string();

    let tif_path = out_dir.join(format!("{tile_id_str}.tif"));
    let tif_file = File::create(&tif_path)
        .with_context(|| format!("Cannot create {}", tif_path.display()))?;
    let mut encoder = TiffEncoder::new(BufWriter::new(tif_file))
        .context("Failed to create TIFF encoder")?;
    encoder
        .write_image::<colortype::Gray16>(TILE_SIDE as u32, TILE_SIDE as u32, &result)
        .with_context(|| format!("Failed to write TIFF {}", tif_path.display()))?;

    Ok(Some(tile_id_str))
}

/// Split a citywide DEM GeoTIFF into canonical 500-usft tiles, streaming strip by strip.
///
/// Each pixel at (row, col) maps to exactly one tile: the tile buffers for one northing
/// band are held in memory at a time and flushed as soon as the band's last row is read.
/// Peak memory is O(image_width × TILE_SIDE × 8 bytes) rather than the full image.
///
/// Input must be a stripped (not tiled) TIFF in EPSG:6539+6360, 1 usft/pixel, heights
/// in US survey feet, with the NW corner at (DEM_ORIGIN_E, DEM_ORIGIN_N).
pub fn split_dem(dem_path: &Path, out_dir: &Path) -> Result<Vec<String>> {
    std::fs::create_dir_all(out_dir)?;

    println!("Reading {} …", dem_path.display());
    let file = File::open(dem_path)
        .with_context(|| format!("Cannot open {}", dem_path.display()))?;
    let mut decoder = Decoder::new(file)
        .context("Failed to open DEM TIFF")?
        .with_limits({
            let mut limits = Limits:: default();
            // Profiling shows this tops out at around 2GB, so this should be plenty of buffer
            limits.decoding_buffer_size = 5 * 1024 * 1024 * 1024;
            limits.intermediate_buffer_size = 5 * 1024 * 1024 * 1024;
            limits
        });

    let (dem_w, dem_h) = decoder.dimensions().context("Failed to read DEM dimensions")?;
    let (dem_w, dem_h) = (dem_w as usize, dem_h as usize);

    println!(
        "DEM size: {dem_w}×{dem_h} pixels  ({:.1}×{:.1} miles)",
        dem_w as f64 / 5280.0,
        dem_h as f64 / 5280.0
    );

    let strip_count = decoder
        .strip_count()
        .context("Failed to get strip count — is the DEM tiled rather than stripped?")?;

    let pb = ProgressBar::new(dem_h as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{bar:40} {pos}/{len} rows | {per_sec} | eta: {eta}")
            .unwrap(),
    );

    // Active tile buffers keyed by (e_sw, n_sw). At most one northing band is live at once.
    let mut tile_buffers: HashMap<(i64, i64), Vec<f64>> = HashMap::new();
    let mut tile_ids: Vec<String> = Vec::new();
    let mut current_row = 0usize;

    for strip_idx in 0..strip_count {
        let strip_data = match decoder
            .read_chunk(strip_idx)
            .with_context(|| format!("Failed to read strip {strip_idx}"))?
        {
            DecodingResult::I32(v) => v,
            other => anyhow::bail!("Expected I32 DEM data, got {:?}", other),
        };

        let rows_in_strip = strip_data.len() / dem_w;

        for row_in_strip in 0..rows_in_strip {
            let row = current_row + row_in_strip;
            if row >= dem_h {
                break;
            }

            // Northing of the SW corner of the pixel at this row.
            let northing = DEM_ORIGIN_N - row as i64 - 1;
            let n_sw = (northing / TILE_SIDE as i64) * TILE_SIDE as i64;
            let y = (northing - n_sw) as usize; // 0 = southernmost row of the tile

            let row_slice = &strip_data[row_in_strip * dem_w..(row_in_strip + 1) * dem_w];
            for (col, &pixel) in row_slice.iter().enumerate() {
                let easting = DEM_ORIGIN_E + col as i64;
                let e_sw = (easting / TILE_SIDE as i64) * TILE_SIDE as i64;
                let x = (easting - e_sw) as usize;

                tile_buffers
                    .entry((e_sw, n_sw))
                    .or_insert_with(|| vec![0.0f64; TILE_SIDE * TILE_SIDE])
                    [x * TILE_SIDE + y] = pixel as f64;
            }

            // y == 0 means this is the southernmost (last) row for this northing band.
            if y == 0 {
                let to_flush: Vec<((i64, i64), Vec<f64>)> = tile_buffers
                    .extract_if(|(_, ns), _| *ns == n_sw)
                    .collect();

                let new_ids: Result<Vec<Option<String>>> = to_flush
                    .into_par_iter()
                    .map(|((e_sw, n_sw), buf)| flush_tile(buf, e_sw, n_sw, out_dir))
                    .collect();
                tile_ids.extend(new_ids?.into_iter().flatten());
            }
        }

        current_row += rows_in_strip;
        pb.set_position(current_row as u64);
    }

    // Flush any partial tiles at the southern edge of the DEM.
    let remaining: Vec<((i64, i64), Vec<f64>)> = tile_buffers.drain().collect();
    let new_ids: Result<Vec<Option<String>>> = remaining
        .into_par_iter()
        .map(|((e_sw, n_sw), buf)| flush_tile(buf, e_sw, n_sw, out_dir))
        .collect();
    tile_ids.extend(new_ids?.into_iter().flatten());

    pb.finish_and_clear();
    tile_ids.sort();
    println!("Done: {} tiles written.", tile_ids.len());
    Ok(tile_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiff::decoder::Decoder as TiffDecoder;

    /// SW corners of all canonical 500-usft tiles that overlap the DEM extent.
    fn dem_tile_sw_corners(dem_h: usize, dem_w: usize) -> Vec<(i64, i64)> {
        let n_min = DEM_ORIGIN_N - dem_h as i64;
        let e_max = DEM_ORIGIN_E + dem_w as i64 - 1;
        let n_max = DEM_ORIGIN_N - 1;

        let e_start = (DEM_ORIGIN_E / TILE_SIDE as i64) * TILE_SIDE as i64;
        let n_start = (n_min / TILE_SIDE as i64) * TILE_SIDE as i64;

        let mut corners = Vec::new();
        let mut e = e_start;
        while e <= e_max {
            let mut n = n_start;
            while n <= n_max {
                corners.push((e, n));
                n += TILE_SIDE as i64;
            }
            e += TILE_SIDE as i64;
        }
        corners
    }

    /// Write a Vec<i32> as a single-strip GrayI32 TIFF and return the path.
    fn write_synthetic_dem(path: &std::path::Path, dem: &[i32], w: usize, h: usize) {
        let f = File::create(path).unwrap();
        let mut enc = TiffEncoder::new(BufWriter::new(f)).unwrap();
        enc.write_image::<colortype::GrayI32>(w as u32, h as u32, dem).unwrap();
    }

    /// Decode a Gray16 TIFF tile written by split_dem back into a Vec<u16>.
    fn read_tile(path: &std::path::Path) -> Vec<u16> {
        let f = File::open(path).unwrap();
        let mut dec = TiffDecoder::new(f).unwrap().with_limits(Limits::unlimited());
        match dec.read_image().unwrap() {
            DecodingResult::U16(v) => v,
            other => panic!("unexpected pixel type {:?}", other),
        }
    }

    /// Verify that split_dem produces tiles identical to extract_dem_tile for every
    /// candidate corner of a synthetic DEM.
    #[test]
    fn streaming_matches_batch_extraction() {
        let dem_h = 600usize;
        let dem_w = 800usize;
        // Unique-ish small positive values so ft×12 fits in u16.
        let dem: Vec<i32> = (0..dem_h * dem_w)
            .map(|i| (i % 100 + 1) as i32)
            .collect();

        let tmp = tempfile::tempdir().unwrap();
        let dem_path = tmp.path().join("synthetic.tif");
        let out_dir = tmp.path().join("tiles");
        std::fs::create_dir_all(&out_dir).unwrap();

        write_synthetic_dem(&dem_path, &dem, dem_w, dem_h);
        split_dem(&dem_path, &out_dir).unwrap();

        for (e_sw, n_sw) in dem_tile_sw_corners(dem_h, dem_w) {
            let expected = extract_dem_tile(&dem, dem_h, dem_w, e_sw, n_sw);
            let center = NYSCoords2::new(e_sw as f64 + 0.5, n_sw as f64 + 0.5);
            let tile_id = TileId::from_contained_point(&center).to_string();
            let tif_path = out_dir.join(format!("{tile_id}.tif"));

            match expected {
                None => assert!(
                    !tif_path.exists(),
                    "tile {tile_id} should not have been written"
                ),
                Some(want) => {
                    assert!(tif_path.exists(), "tile {tile_id} was not written");
                    let got = read_tile(&tif_path);
                    assert_eq!(got, want, "pixel mismatch in tile {tile_id}");
                }
            }
        }
    }

    #[test]
    fn extract_tile_dims_are_always_500x500() {
        // Synthetic 600×600 DEM, all 1.0 ft.
        let dem: Vec<i32> = vec![1; 600 * 600];
        let result = extract_dem_tile(&dem, 600, 600, DEM_ORIGIN_E, DEM_ORIGIN_N - 500);
        // A tile right at the NW corner of the DEM should be found.
        if let Some(raster) = result {
            assert_eq!(raster.len(), TILE_SIDE * TILE_SIDE);
        }
        // Tile fully outside should return None.
        assert!(extract_dem_tile(&dem, 600, 600, DEM_ORIGIN_E + 10_000, DEM_ORIGIN_N).is_none());
    }

    #[test]
    fn heights_converted_to_inches() {
        // DEM with height=10ft covering the output tile fully.
        let h = 1100; // pixels tall to guarantee overlap
        let w = 1100;
        let dem: Vec<i32> = vec![10; h * w];
        let e_sw = DEM_ORIGIN_E;
        let n_sw = DEM_ORIGIN_N - TILE_SIDE as i64;

        let raster = extract_dem_tile(&dem, h, w, e_sw, n_sw).expect("tile should exist");
        // 10 ft * 12 = 120 inches
        assert!(raster.iter().all(|&v| v == 120), "expected all pixels = 120, got {:?}", &raster[..4]);
    }

    #[test]
    fn all_zero_tile_is_skipped() {
        let h = 600;
        let w = 600;
        let dem: Vec<i32> = vec![0; h * w];
        let result = extract_dem_tile(&dem, h, w, DEM_ORIGIN_E, DEM_ORIGIN_N - 500);
        assert!(result.is_none(), "all-zero tile should return None");
    }

    #[test]
    fn sw_corners_cover_dem_extent() {
        let dem_h = 1000usize;
        let dem_w = 1000usize;
        let corners = dem_tile_sw_corners(dem_h, dem_w);
        // Every corner's e_sw should be <= DEM_ORIGIN_E + dem_w - 1.
        for (e, _) in &corners {
            assert!(*e <= DEM_ORIGIN_E + dem_w as i64 - 1);
        }
        assert!(!corners.is_empty());
    }
}