use crate::building::bin_id::BINId;
use crate::providers::elevation_tile_provider::ElevationTileProvider;
use crate::providers::footprint_provider::FootprintProvider;
use crate::types::coords::{NYSCoords2, valid_nys_coordinate};
use crate::types::errors::{AssetErr};
use crate::types::tiles::{SUBGRID_TILE_SIDE_LENGTH_USFT, TileId};
use derive_getters::Getters;
use derive_new::new;
use geo::{BoundingRect, Buffer, Contains, Convert, Intersects, Polygon, Rect, point};
use ndarray::{Array2, Zip, s};
use rocket::http::Status;
use std::cmp::{max, min};
use std::convert::TryInto;

const MAX_TILES_PER_BUILDING_FOOTPRINT: u16 = 500;
const FILTER_DISTANCE_Z_USFT: f64 = 15.0;

#[derive(Debug)]
pub enum HeightMapCreateErr {
    AssetErr(AssetErr),
    NoTiles(String),
    InvalidFootprint(String),
}

impl From<AssetErr> for HeightMapCreateErr {
    fn from(e: AssetErr) -> HeightMapCreateErr {
        HeightMapCreateErr::AssetErr(e)
    }
}

impl From<HeightMapCreateErr> for Status {
    fn from(e: HeightMapCreateErr) -> Status {
        match e {
            HeightMapCreateErr::NoTiles(_) => Status::NoContent,
            HeightMapCreateErr::InvalidFootprint(_) => Status::UnprocessableEntity,
            HeightMapCreateErr::AssetErr(ae) => Status::from(ae),
        }
    }
}

#[derive(Debug, new, Getters)]
pub struct RooftopHeightMap {
    bin_id: BINId,
    sw_offset: NYSCoords2,

    // Values are in inches above the NY SP Long Island datum,
    // axes are [easting_local, northing_local] (add sw_offset to get global position)
    // Pixels outside the mask=true footprint are set to 0
    heightmap: Array2<u16>,

    // A mask over the dimensions of heightmap, where true, the height is valid,
    // where false, it's not
    mask: Array2<bool>,

    // The shape of the underlying building footprint in NY SP LI coordinates
    poly_nys: Polygon,
}

#[derive(new)]
pub struct RooftopHeightMapFactory<'a> {
    footprint_provider: &'a (dyn FootprintProvider + Send + Sync),
    elevation_tile_provider: &'a (dyn ElevationTileProvider + Send + Sync),
}

impl<'a> RooftopHeightMapFactory<'a> {
    pub async fn create(&self, bin_id: BINId) -> Result<RooftopHeightMap, HeightMapCreateErr> {
        let footprint = self.footprint_provider.get_footprint(bin_id).await?;
        let (intersecting_tiles, poly_bounds) = get_intersecting_tiles(&footprint)?;

        if intersecting_tiles.is_empty() {
            return Err(HeightMapCreateErr::NoTiles(format!(
                "No preprocessed tiles intersect the specified bin: {bin_id:?}"
            )));
        }

        let (poly_w, poly_s) = poly_bounds.min().x_y();
        let (poly_e, poly_n) = poly_bounds.max().x_y();

        let sw_corner = NYSCoords2::new(poly_w, poly_s);

        // Safety: The as casts and unwraps are safe because get_intersecting_tiles()
        // validates the polygon bounds and returns Err() if any are too big or small
        let poly_w: u32 = (poly_w.floor() as i64).try_into().unwrap();
        let poly_s: u32 = (poly_s.floor() as i64).try_into().unwrap();
        let poly_e: u32 = (poly_e.ceil() as i64).try_into().unwrap();
        let poly_n: u32 = (poly_n.ceil() as i64).try_into().unwrap();

        // Safety: these unwraps are safe on all platforms where usize >= u32
        let output_h: usize = (poly_n - poly_s).try_into().unwrap();
        let output_w: usize = (poly_e - poly_w).try_into().unwrap();

        let mut heightmap = Array2::<u16>::zeros((output_w, output_h));

        for tile_id in intersecting_tiles {
            // Compute intersection between tile data
            let tile_bounds = tile_id.get_bounds();
            let (tile_w, tile_s) = tile_bounds.min().x_y();
            let (tile_e, tile_n) = tile_bounds.max().x_y();

            let nys_w = max(poly_w, tile_w);
            let nys_e = min(poly_e, tile_e);

            if nys_w >= nys_e {
                continue;
            }

            let nys_s = max(poly_s, tile_s);
            let nys_n = min(poly_n, tile_n);

            // Compute relative location of intersection slice in output grid
            // Safety: these unwraps are safe on all platforms where usize >= u32
            let out_x_start: usize = (nys_w - poly_w).try_into().unwrap();
            let out_x_end: usize = (nys_e - poly_w).try_into().unwrap();
            let out_y_start: usize = (nys_s - poly_s).try_into().unwrap();
            let out_y_end: usize = (nys_n - poly_s).try_into().unwrap();

            // Compute relative location of intersection slice in tile grid
            // Safety: these unwraps are safe on all platforms where usize >= u32
            let tile_x_start: usize = (nys_w - tile_w).try_into().unwrap();
            let tile_x_end: usize = (nys_e - tile_w).try_into().unwrap();
            let tile_y_start: usize = (nys_s - tile_s).try_into().unwrap();
            let tile_y_end: usize = (nys_n - tile_s).try_into().unwrap();

            let tile = self
                .elevation_tile_provider
                .get_elevation_tile(tile_id)
                .await?;
            let tile_contents = tile.elevation_inches();

            // Read the tile contents into the heightmap in the appropriate spot
            heightmap
                .slice_mut(s![out_x_start..out_x_end, out_y_start..out_y_end])
                .assign(
                    &*tile_contents.slice(s![tile_x_start..tile_x_end, tile_y_start..tile_y_end,]),
                );
        }

        let buffered_footprint = footprint.buffer(0.5);
        let mask = Array2::<bool>::from_shape_fn((output_w, output_h), |(x, y)| {
            buffered_footprint.contains(
                // Unwraps are safe on all platforms where usize >= u32, as f64 is safe because
                // this whole expression is bounded by get_intersecting_tiles' boundary validations
                &point! {
                x: (usize::try_from(poly_w).unwrap() + x) as f64,
                y: (usize::try_from(poly_s).unwrap() + y) as f64},
            )
        });

        // Technically we could just output the original heightmap here instead of doing this
        //  O(N) overwrite, since callers aren't supposed to rely on the contents of
        //  anything where mask is false, but we're nice so we won't for now
        Zip::from(&mut heightmap)
            .and(&mask)
            .for_each(|val: &mut u16, m: &bool| {
                if !m {
                    *val = 0
                }
            });

        // Gently smooth out the generated heightmap to reduce noise due to building edges and
        // missing data squares
        filter_heightmap_outliers(&mut heightmap, &mask);

        Ok(RooftopHeightMap::new(
            bin_id, sw_corner, heightmap, mask, footprint,
        ))
    }
}

fn filter_heightmap_outliers(heightmap: &mut Array2<u16>, mask: &Array2<bool>) {
    const THRESHOLD_INCHES: f64 = FILTER_DISTANCE_Z_USFT * 12.0;
    let original = heightmap.clone();
    let (nrows, ncols) = (heightmap.nrows(), heightmap.ncols());

    for xi in 0..nrows {
        for yi in 0..ncols {
            if !mask[[xi, yi]] {
                continue;
            }

            let mut neighbor_sum = 0.0f64;
            let mut neighbor_count = 0u32;
            for dxi in [-1isize, 0, 1] {
                for dyi in [-1isize, 0, 1] {
                    if dxi == 0 && dyi == 0 {
                        continue;
                    }
                    let Some(nx) = xi.checked_add_signed(dxi) else {
                        continue;
                    };
                    let Some(ny) = yi.checked_add_signed(dyi) else {
                        continue;
                    };
                    if nx >= nrows || ny >= ncols {
                        continue;
                    }
                    if !mask[[nx, ny]] {
                        continue;
                    }
                    neighbor_sum += f64::from(original[[nx, ny]]);
                    neighbor_count += 1;
                }
            }

            if neighbor_count == 0 {
                continue;
            }
            let neighbor_avg = neighbor_sum / f64::from(neighbor_count);
            if (f64::from(original[[xi, yi]]) - neighbor_avg).abs() > THRESHOLD_INCHES {
                heightmap[[xi, yi]] = neighbor_avg.round() as u16;
            }
        }
    }
}

pub fn get_intersecting_tiles(
    poly_nys: &Polygon,
) -> Result<(Vec<TileId>, Rect), HeightMapCreateErr> {
    let bounding_rect = poly_nys.bounding_rect().ok_or_else(|| {
        HeightMapCreateErr::InvalidFootprint(format!(
            "Invalid footprint, must have defined bounding_rect: {poly_nys:?}"
        ))
    })?;

    let (w, s) = bounding_rect.min().x_y();
    let (e, n) = bounding_rect.max().x_y();

    if !valid_nys_coordinate(w)
        || !valid_nys_coordinate(e)
        || !valid_nys_coordinate(s)
        || !valid_nys_coordinate(n)
    {
        return Err(HeightMapCreateErr::InvalidFootprint(format!(
            "Invalid footprint, does not fit in NYS plane (bounding box: ({w}{s}) ({e}{n}))"
        )));
    }

    let subgrid_tile_side_length_usft_f64: f64 = SUBGRID_TILE_SIDE_LENGTH_USFT.into();

    let height_usft = n - s;
    let width_usft = e - w;

    // Safety: no rollover, since the max output of .ceil() here is 4000
    let height = (height_usft / subgrid_tile_side_length_usft_f64).ceil() as u64;
    let width = (width_usft / subgrid_tile_side_length_usft_f64).ceil() as u64;

    // No roll over here since max(width, height) is ~2 million
    let candidate_tiles_count = height * width;

    if candidate_tiles_count > u64::from(MAX_TILES_PER_BUILDING_FOOTPRINT) {
        return Err(HeightMapCreateErr::InvalidFootprint(format!(
            "Invalid footprint, too big! Expected fewer than {} tiles but found {}",
            MAX_TILES_PER_BUILDING_FOOTPRINT, candidate_tiles_count
        )));
    }

    // Safety: this unwrap() will never panic, since we asserted above that
    // candidate_tiles_count <= MAX_TILES_PER_BUILDING_FOOTPRINT which is <= max(u16) <= max(usize)
    let mut intersecting_tiles = Vec::with_capacity(candidate_tiles_count.try_into().unwrap());

    // Bias to an epsilon before the bounding box in our point sampling, to account
    // for the degenerate case of a polygon that starts exactly on the border
    let mut cursor_n = s - 1.0;
    loop {
        let mut cursor_e = w - 1.0;
        loop {
            let sample_point = NYSCoords2::new(cursor_e, cursor_n);
            let tile_id = TileId::from_contained_point(&sample_point);
            if poly_nys.intersects(&tile_id.get_bounds().convert()) {
                intersecting_tiles.push(tile_id);
            }
            if cursor_e > e {
                break;
            }
            cursor_e += subgrid_tile_side_length_usft_f64;
        }
        if cursor_n > n {
            break;
        }
        cursor_n += subgrid_tile_side_length_usft_f64;
    }

    Ok((intersecting_tiles, bounding_rect))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::elevation_tile_provider::{ElevationTile, ElevationTileProvider};
    use crate::providers::footprint_provider::FootprintProvider;
    use async_trait::async_trait;
    use geo::polygon;
    use std::collections::HashSet;

    fn rect_poly(x0: f64, y0: f64, x1: f64, y1: f64) -> Polygon {
        polygon![
            (x: x0, y: y0),
            (x: x1, y: y0),
            (x: x1, y: y1),
            (x: x0, y: y1),
            (x: x0, y: y0),
        ]
    }

    // --- filter_heightmap_outliers ---

    fn uniform_mask(shape: (usize, usize), val: bool) -> Array2<bool> {
        Array2::from_elem(shape, val)
    }

    #[test]
    fn filter_replaces_outlier_with_neighbor_average() {
        // 3×3 grid, centre pixel is a clear outlier (500 in ≈ 41.7 ft above neighbours at 100 in).
        // All pixels are masked. The 8 neighbours all equal 100, average = 100.
        // 500 - 100 = 400 in = 33.3 ft > 15 ft threshold → centre replaced with 100.
        let mut hm = Array2::<u16>::from_elem((3, 3), 100);
        hm[[1, 1]] = 500;
        let mask = uniform_mask((3, 3), true);
        filter_heightmap_outliers(&mut hm, &mask);
        assert_eq!(
            hm[[1, 1]],
            100,
            "outlier should be replaced by neighbour average"
        );
        assert_eq!(
            hm[[0, 0]],
            100,
            "non-outlier neighbours should be unchanged"
        );
    }

    #[test]
    fn filter_leaves_pixel_within_threshold_unchanged() {
        // Centre pixel is 1500 in = 125 ft; neighbours are 1440 in = 120 ft.
        // Difference = 60 in = 5 ft < 15 ft → no change.
        let mut hm = Array2::<u16>::from_elem((3, 3), 1440);
        hm[[1, 1]] = 1500;
        let mask = uniform_mask((3, 3), true);
        filter_heightmap_outliers(&mut hm, &mask);
        assert_eq!(
            hm[[1, 1]],
            1500,
            "pixel within threshold should not be replaced"
        );
    }

    #[test]
    fn filter_leaves_isolated_masked_pixel_unchanged() {
        // Only the centre pixel is masked; it has no masked neighbours → no change.
        let mut hm = Array2::<u16>::from_elem((3, 3), 0);
        hm[[1, 1]] = 999;
        let mut mask = uniform_mask((3, 3), false);
        mask[[1, 1]] = true;
        filter_heightmap_outliers(&mut hm, &mask);
        assert_eq!(
            hm[[1, 1]],
            999,
            "pixel with no masked neighbours should not change"
        );
    }

    #[test]
    fn filter_does_not_touch_unmasked_pixels() {
        // Corner pixel is unmasked and looks like an outlier — filter must ignore it.
        let mut hm = Array2::<u16>::from_elem((3, 3), 100);
        hm[[0, 0]] = 5000;
        let mut mask = uniform_mask((3, 3), true);
        mask[[0, 0]] = false;
        filter_heightmap_outliers(&mut hm, &mask);
        assert_eq!(hm[[0, 0]], 5000, "unmasked pixel must not be modified");
    }

    #[test]
    fn filter_average_excludes_unmasked_neighbours() {
        // Centre pixel = 1000 in; its 8 neighbours are all 100 in, but half are unmasked.
        // Masked neighbours: 4 pixels at 100 in → avg = 100. Diff = 900 in = 75 ft > 15 ft.
        let mut hm = Array2::<u16>::from_elem((3, 3), 100);
        hm[[1, 1]] = 1000;
        let mut mask = uniform_mask((3, 3), true);
        mask[[0, 0]] = false;
        mask[[0, 2]] = false;
        mask[[2, 0]] = false;
        mask[[2, 2]] = false;
        filter_heightmap_outliers(&mut hm, &mask);
        assert_eq!(
            hm[[1, 1]],
            100,
            "outlier replaced using only masked neighbours"
        );
    }

    // --- get_intersecting_tiles ---

    #[test]
    fn get_intersecting_tiles_single_tile() {
        // Small square entirely within subgrid tile 500300_00 (bounds: 500000-500500, 300000-300500)
        let poly = rect_poly(500100.0, 300100.0, 500200.0, 300200.0);
        let (tiles, _) = get_intersecting_tiles(&poly).unwrap();
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].to_string(), "500300_00");
    }

    #[test]
    fn get_intersecting_tiles_single_tile_degenerate_poly() {
        let poly = rect_poly(500100.0, 300100.0, 500100.0, 300100.0);
        let (tiles, _) = get_intersecting_tiles(&poly).unwrap();
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].to_string(), "500300_00");
    }

    #[test]
    fn get_intersecting_tiles_single_tile_sw_of_corner() {
        let corner_point = TileId::parse("500300_11").unwrap().get_sw_corner();
        let poly = rect_poly(
            corner_point.easting() - 2.0,
            corner_point.northing() - 2.0,
            corner_point.easting() - 1.0,
            corner_point.northing() - 1.0,
        );
        let (tiles, _) = get_intersecting_tiles(&poly).unwrap();
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].to_string(), "500300_00");
    }

    #[test]
    fn get_intersecting_tiles_single_tile_ne_of_corner() {
        let corner_point = TileId::parse("500300_11").unwrap().get_sw_corner();
        let poly = rect_poly(
            corner_point.easting() + 1.0,
            corner_point.northing() + 1.0,
            corner_point.easting() + 2.0,
            corner_point.northing() + 2.0,
        );
        let (tiles, _) = get_intersecting_tiles(&poly).unwrap();
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].to_string(), "500300_11");
    }

    #[test]
    fn get_intersecting_tiles_spans_two_subgrids_and_las_tiles() {
        let poly = rect_poly(989800.0, 200100.0, 990100.0, 200200.0);
        let (tiles, _) = get_intersecting_tiles(&poly).unwrap();
        let tile_strs: HashSet<String> = tiles.iter().map(|t| t.to_string()).collect();
        assert_eq!(tile_strs.len(), 2);
        assert!(tile_strs.contains("987200_40"));
        assert!(tile_strs.contains("990200_00"));
    }

    #[test]
    fn get_intersecting_tiles_spans_three_subgrids_on_two_las_tiles() {
        let poly = rect_poly(989800.0, 200100.0, 990600.0, 200200.0);
        let (tiles, _) = get_intersecting_tiles(&poly).unwrap();
        let tile_strs: HashSet<String> = tiles.iter().map(|t| t.to_string()).collect();
        assert_eq!(tile_strs.len(), 3);
        assert!(tile_strs.contains("987200_40"));
        assert!(tile_strs.contains("990200_00"));
        assert!(tile_strs.contains("990200_10"));
    }

    #[test]
    fn get_intersecting_tiles_spans_four_subgrids() {
        // Rectangle from (500200, 300200) to (500800, 300800) crosses four subgrid tiles:
        // 500300_00, 500300_10, 500300_01, 500300_11
        let poly = rect_poly(500200.0, 300200.0, 500800.0, 300800.0);
        let (tiles, _) = get_intersecting_tiles(&poly).unwrap();
        let tile_strs: HashSet<String> = tiles.iter().map(|t| t.to_string()).collect();
        assert_eq!(tile_strs.len(), 4);
        assert!(tile_strs.contains("500300_00"));
        assert!(tile_strs.contains("500300_10"));
        assert!(tile_strs.contains("500300_01"));
        assert!(tile_strs.contains("500300_11"));
    }

    #[test]
    fn get_intersecting_tiles_degenerate_poly_on_corner() {
        let poly = rect_poly(990000.0, 190000.0, 990000.0, 190000.0);
        let (tiles, _) = get_intersecting_tiles(&poly).unwrap();
        let tile_strs: HashSet<String> = tiles.iter().map(|t| t.to_string()).collect();
        assert_eq!(tile_strs.len(), 4);
        assert!(tile_strs.contains("987190_40"));
        assert!(tile_strs.contains("987187_44"));
        assert!(tile_strs.contains("990187_04"));
        assert!(tile_strs.contains("990190_00"));
    }

    #[test]
    fn get_intersecting_tiles_spans_four_subgrids_and_las_tiles_just_barely() {
        let corner_point = TileId::parse("500300_00").unwrap().get_sw_corner();
        let poly = rect_poly(
            corner_point.easting() - 1.0,
            corner_point.northing() - 1.0,
            corner_point.easting() + 1.0,
            corner_point.northing() + 1.0,
        );
        let (tiles, _) = get_intersecting_tiles(&poly).unwrap();
        let tile_strs: HashSet<String> = tiles.iter().map(|t| t.to_string()).collect();
        assert_eq!(tile_strs.len(), 4);
        assert!(tile_strs.contains("497297_44"));
        assert!(tile_strs.contains("500297_04"));
        assert!(tile_strs.contains("497300_40"));
        assert!(tile_strs.contains("500300_00"));
    }

    #[test]
    fn get_intersecting_tiles_returns_bounding_rect() {
        let poly = rect_poly(500100.0, 300100.0, 500200.0, 300200.0);
        let (_, bounds) = get_intersecting_tiles(&poly).unwrap();
        assert_eq!(bounds.min().x_y(), (500100.0, 300100.0));
        assert_eq!(bounds.max().x_y(), (500200.0, 300200.0));
    }

    #[test]
    fn get_intersecting_tiles_out_of_nys_bounds() {
        // Polygon with a negative easting coordinate
        let poly = rect_poly(-100.0, 300100.0, 100.0, 300200.0);
        assert!(matches!(
            get_intersecting_tiles(&poly),
            Err(HeightMapCreateErr::InvalidFootprint(_))
        ));
    }

    #[test]
    fn get_intersecting_tiles_exceeds_max_tile_count() {
        // 12000×12000 usft → ceil(12000/500)^2 = 576 candidate tiles > MAX_TILES_PER_BUILDING_FOOTPRINT (500)
        let poly = rect_poly(500000.0, 300000.0, 512000.0, 312000.0);
        assert!(matches!(
            get_intersecting_tiles(&poly),
            Err(HeightMapCreateErr::InvalidFootprint(_))
        ));
    }

    // --- RooftopHeightMapFactory::create ---

    // The mock elevation tile is a 500×500 grid divided into four quadrants by
    // elevation value.  Row/col < 250 is the south/west half of the tile.
    //
    //   col →   west (0-249)   east (250-499)
    //   row ↑
    //   north   Q3 = 300       Q4 = 400
    //   south   Q1 = 100       Q2 = 200
    //
    // Tile 500300_00 SW corner is (500000, 300000), so:
    //   Q1/Q2 boundary is at northing 300250  (tile row 250)
    //   Q1/Q3 boundary is at easting  500250  (tile col 250)
    const Q1: u16 = 100;
    const Q2: u16 = 200;
    const Q3: u16 = 300;
    const Q4: u16 = 400;

    struct MockFootprintProvider {
        result: Result<Polygon, AssetErr>,
    }
    struct MockElevationTileProvider {
        result: Result<(), AssetErr>,
    }

    fn clone_asset_err(e: &AssetErr) -> AssetErr {
        match e {
            AssetErr::AssetNotFound(s) => AssetErr::AssetNotFound(s.clone()),
            AssetErr::AssetDownloadError(s) => AssetErr::AssetDownloadError(s.clone()),
            AssetErr::LocalFileSystemError(s) => AssetErr::LocalFileSystemError(s.clone()),
            AssetErr::UnsupportedAssetType(s) => AssetErr::UnsupportedAssetType(s.clone()),
            AssetErr::AssetContentError(s) => AssetErr::AssetContentError(s.clone()),
        }
    }

    #[async_trait]
    impl FootprintProvider for MockFootprintProvider {
        async fn get_footprint(&self, _: BINId) -> Result<Polygon, AssetErr> {
            match &self.result {
                Ok(p) => Ok(p.clone()),
                Err(e) => Err(clone_asset_err(e)),
            }
        }
    }

    #[async_trait]
    impl ElevationTileProvider for MockElevationTileProvider {
        async fn get_elevation_tile(&self, tile_id: TileId) -> Result<ElevationTile, AssetErr> {
            match &self.result {
                Err(e) => Err(clone_asset_err(e)),
                Ok(()) => {
                    let side = usize::from(SUBGRID_TILE_SIDE_LENGTH_USFT);
                    let data = ndarray::Array2::from_shape_fn((side, side), |(row, col)| {
                        match (row >= side / 2, col >= side / 2) {
                            (false, false) => Q1,
                            (false, true) => Q2,
                            (true, false) => Q3,
                            (true, true) => Q4,
                        }
                    });
                    Ok(ElevationTile::new(tile_id, data))
                }
            }
        }
    }

    fn factory<'a>(
        fp: &'a MockFootprintProvider,
        et: &'a MockElevationTileProvider,
    ) -> RooftopHeightMapFactory<'a> {
        RooftopHeightMapFactory::new(fp, et)
    }

    fn test_bin() -> BINId {
        BINId::parse("1000001").unwrap()
    }

    // 100×100 usft square, fully within tile 500300_00 Q1 (south-west quadrant)
    // → all masked pixels carry elevation Q1=100
    const TEST_W: f64 = 500100.0;
    const TEST_S: f64 = 300100.0;
    const TEST_E: f64 = 500200.0;
    const TEST_N: f64 = 300200.0;

    fn test_poly() -> Polygon {
        rect_poly(TEST_W, TEST_S, TEST_E, TEST_N)
    }

    #[tokio::test]
    async fn create_heightmap_has_correct_dimensions() {
        let fp = MockFootprintProvider {
            result: Ok(test_poly()),
        };
        let et = MockElevationTileProvider { result: Ok(()) };
        let hm = factory(&fp, &et).create(test_bin()).await.unwrap();
        assert_eq!(hm.heightmap().nrows(), (TEST_N - TEST_S) as usize);
        assert_eq!(hm.heightmap().ncols(), (TEST_E - TEST_W) as usize);
    }

    #[tokio::test]
    async fn create_sw_corner_matches_polygon_sw() {
        let fp = MockFootprintProvider {
            result: Ok(test_poly()),
        };
        let et = MockElevationTileProvider { result: Ok(()) };
        let hm = factory(&fp, &et).create(test_bin()).await.unwrap();
        assert_eq!(*hm.sw_offset().easting(), TEST_W);
        assert_eq!(*hm.sw_offset().northing(), TEST_S);
    }

    #[tokio::test]
    async fn create_mask_all_true_for_rect_polygon() {
        let fp = MockFootprintProvider {
            result: Ok(test_poly()),
        };
        let et = MockElevationTileProvider { result: Ok(()) };
        let hm = factory(&fp, &et).create(test_bin()).await.unwrap();
        assert!(hm.mask().iter().all(|&m| m));
        // Entire polygon is in Q1, so all pixels should carry that elevation
        assert!(hm.heightmap().iter().all(|&v| v == Q1));
    }

    #[tokio::test]
    async fn create_mask_shape_matches_heightmap() {
        let fp = MockFootprintProvider {
            result: Ok(test_poly()),
        };
        let et = MockElevationTileProvider { result: Ok(()) };
        let hm = factory(&fp, &et).create(test_bin()).await.unwrap();
        assert_eq!(hm.mask().dim(), hm.heightmap().dim());
    }

    #[tokio::test]
    async fn create_mask_rhombus_footprint() {
        // Rhombus with vertices at the midpoints of the test square's sides.
        // The bounding box is the same 100×100 square so the heightmap dimensions
        // are unchanged, but the corners of the bounding box fall outside the
        // footprint and the center falls inside.
        let rhombus = polygon![
            (x: TEST_W + 50.0, y: TEST_S),         // bottom mid
            (x: TEST_E,        y: TEST_S + 50.0),  // right mid
            (x: TEST_W + 50.0, y: TEST_N),         // top mid
            (x: TEST_W,        y: TEST_S + 50.0),  // left mid
            (x: TEST_W + 50.0, y: TEST_S),         // close
        ];
        let fp = MockFootprintProvider {
            result: Ok(rhombus),
        };
        let et = MockElevationTileProvider { result: Ok(()) };
        let hm = factory(&fp, &et).create(test_bin()).await.unwrap();

        assert_eq!(hm.heightmap().dim(), (100, 100));

        // Center and bounding-box corners — mask and elevation
        assert!(hm.mask()[[50, 50]], "center should be inside the rhombus");
        assert_eq!(hm.heightmap()[[50, 50]], Q1, "center is in Q1");
        assert!(
            !hm.mask()[[0, 0]],
            "SW corner should be outside the rhombus"
        );
        assert_eq!(hm.heightmap()[[0, 0]], 0, "outside pixels are zeroed");
        assert!(
            !hm.mask()[[0, 99]],
            "SE corner should be outside the rhombus"
        );
        assert!(
            !hm.mask()[[99, 0]],
            "NW corner should be outside the rhombus"
        );
        assert!(
            !hm.mask()[[99, 99]],
            "NE corner should be outside the rhombus"
        );
        assert!(
            !hm.mask().iter().all(|&m| m),
            "not all pixels should be masked"
        );

        // Points fuzzed either side of the midpoint of each rhombus edge.
        // Each edge midpoint sits exactly on the boundary at L1 distance 50 from
        // the centre; one step inward (L1 dist 48) is inside, one step outward
        // (L1 dist 52) is outside.
        assert!(hm.mask()[[26, 26]], "just inside bottom-left edge midpoint");
        assert_eq!(hm.heightmap()[[26, 26]], Q1);
        assert!(
            !hm.mask()[[24, 24]],
            "just outside bottom-left edge midpoint"
        );
        assert_eq!(hm.heightmap()[[24, 24]], 0);
        assert!(
            hm.mask()[[74, 26]],
            "just inside bottom-right edge midpoint"
        );
        assert_eq!(hm.heightmap()[[74, 26]], Q1);
        assert!(
            !hm.mask()[[76, 24]],
            "just outside bottom-right edge midpoint"
        );
        assert_eq!(hm.heightmap()[[76, 24]], 0);
        assert!(hm.mask()[[26, 74]], "just inside top-left edge midpoint");
        assert_eq!(hm.heightmap()[[26, 74]], Q1);
        assert!(!hm.mask()[[24, 76]], "just outside top-left edge midpoint");
        assert_eq!(hm.heightmap()[[24, 76]], 0);
        assert!(hm.mask()[[74, 74]], "just inside top-right edge midpoint");
        assert_eq!(hm.heightmap()[[74, 74]], Q1);
        assert!(!hm.mask()[[76, 76]], "just outside top-right edge midpoint");
        assert_eq!(hm.heightmap()[[76, 76]], 0);
    }

    #[tokio::test]
    async fn create_heightmap_stitched_across_quadrants() {
        // 300×300 polygon spanning all four tile quadrants within tile 500300_00.
        // Tile quadrant boundaries: northing 300250 (row 250), easting 500250 (col 250).
        //
        // Output layout (row axis = northing, col axis = easting):
        //   rows   0-149, cols   0-149  → tile rows 100-249, cols 100-249  → Q1
        //   rows   0-149, cols 150-299  → tile rows 100-249, cols 250-399  → Q2
        //   rows 150-299, cols   0-149  → tile rows 250-399, cols 100-249  → Q3
        //   rows 150-299, cols 150-299  → tile rows 250-399, cols 250-399  → Q4
        let poly = rect_poly(500100.0, 300100.0, 500400.0, 300400.0);
        let fp = MockFootprintProvider { result: Ok(poly) };
        let et = MockElevationTileProvider { result: Ok(()) };
        let hm = factory(&fp, &et).create(test_bin()).await.unwrap();

        assert_eq!(hm.heightmap().dim(), (300, 300));

        assert_eq!(hm.heightmap()[[75, 75]], Q1, "Q1 centre");
        assert_eq!(hm.heightmap()[[75, 225]], Q2, "Q2 centre");
        assert_eq!(hm.heightmap()[[225, 75]], Q3, "Q3 centre");
        assert_eq!(hm.heightmap()[[225, 225]], Q4, "Q4 centre");

        // Boundary rows/cols should switch at output index 150
        assert_eq!(hm.heightmap()[[149, 149]], Q1, "just inside Q1 at boundary");
        assert_eq!(hm.heightmap()[[149, 150]], Q2, "just inside Q2 at boundary");
        assert_eq!(hm.heightmap()[[150, 149]], Q3, "just inside Q3 at boundary");
        assert_eq!(hm.heightmap()[[150, 150]], Q4, "just inside Q4 at boundary");
    }

    #[tokio::test]
    async fn create_propagates_footprint_provider_error() {
        let fp = MockFootprintProvider {
            result: Err(AssetErr::AssetNotFound("no footprint".into())),
        };
        let et = MockElevationTileProvider { result: Ok(()) };
        let result = factory(&fp, &et).create(test_bin()).await;
        assert!(matches!(result, Err(HeightMapCreateErr::AssetErr(_))));
    }

    #[tokio::test]
    async fn create_propagates_elevation_tile_error() {
        let fp = MockFootprintProvider {
            result: Ok(test_poly()),
        };
        let et = MockElevationTileProvider {
            result: Err(AssetErr::AssetDownloadError("network failure".into())),
        };
        let result = factory(&fp, &et).create(test_bin()).await;
        assert!(matches!(result, Err(HeightMapCreateErr::AssetErr(_))));
    }
}
