use crate::analysis::fresnel_zone::FresnelZone;
use crate::providers::elevation_tile_provider::{ElevationTile, ElevationTileProvider};
use crate::providers::obstruction_provider::ObstructionProvider;
use crate::types::coords::NYSCoords2;
use crate::types::errors::AssetErr;
use crate::types::obstructions::{
    ObstructionId, ObstructionMeta, ObstructionRaster, ObstructionType, ObstructionTypesFilter,
};
use crate::types::stairstep::StairStepGrid;
use crate::types::tiles::{SUBGRID_TILE_SIDE_LENGTH_USFT, TileId};
use derive_new::new;
use futures_util::{StreamExt, TryStreamExt, stream};
use ndarray::{Array2, ArrayView2, s};
use std::collections::{HashSet};

const PER_LOAD_TILES_CALL_CONCURRENCY_LIMIT_TILES: usize = 10;
const PER_LOAD_TILES_CALL_CONCURRENCY_LIMIT_OBSTRUCTIONS: usize = 30;

pub(crate) type TerrainGrid = StairStepGrid<u16>;

pub fn get_intersecting_tiles(fresnel_zone: &FresnelZone) -> HashSet<TileId> {
    let max_width = (fresnel_zone.values().ncols() as f64
        / f64::from(SUBGRID_TILE_SIDE_LENGTH_USFT))
    .ceil() as usize
        + 1;
    let max_height = (fresnel_zone.values().nrows() as f64
        / f64::from(SUBGRID_TILE_SIDE_LENGTH_USFT))
    .ceil() as usize
        + 1;
    let max_tiles_count = max_width * max_height;

    let mut intersecting_tiles = HashSet::with_capacity(max_tiles_count);

    fresnel_zone
        .widths()
        .iter()
        .zip(fresnel_zone.offsets())
        .enumerate()
        .for_each(|(i, (width, offset))| {
            if *width == 0 {
                return;
            }
            let sample_point = NYSCoords2::new(
                fresnel_zone.base_offset().easting() + *offset as f64,
                fresnel_zone.base_offset().northing() + i as f64,
            );
            let west_tile = TileId::from_contained_point(&sample_point);
            let west_tile_e_edge =
                *west_tile.get_sw_corner().easting() + SUBGRID_TILE_SIDE_LENGTH_USFT as f64;

            let row_e_edge = *sample_point.easting() + *width as f64;
            let remainder = row_e_edge - west_tile_e_edge;
            let mut tile_count_offset = 0;
            loop {
                let tile_id = TileId::from_adjacent_tile(&west_tile, (tile_count_offset, 0));
                intersecting_tiles.insert(tile_id);
                if tile_count_offset as f64 * SUBGRID_TILE_SIDE_LENGTH_USFT as f64 >= remainder {
                    break;
                }
                tile_count_offset += 1;
            }
        });

    intersecting_tiles
}

// Shared blit logic for any source with [easting_local, northing_local] axes.
// src_base is the (easting, northing) of the source's SW corner.
fn bilt_impl(
    src_base: (usize, usize),
    src: ArrayView2<u16>,
    height_values: &mut Array2<u16>,
    zone: &FresnelZone,
) {
    let zone_base = (
        zone.base_offset().easting().floor() as usize,
        zone.base_offset().northing().floor() as usize,
    );

    let src_easting_size = src.nrows();
    let src_northing_size = src.ncols();

    let i_start = (src_base.1 as isize - zone_base.1 as isize).max(0) as usize;
    let Ok(i_end) = ((src_base.1 as isize - zone_base.1 as isize) + src_northing_size as isize)
        .min(zone.widths().len() as isize)
        .try_into()
    else {
        // If i_end < 0, there's no overlap between src and zone, so nothing to bilt.
        // This is a valid scenario, it happens when we try to bilt obstructions into the zone
        // that we identified by being in the same tile as the zone (but not necessarily
        // intersecting it)
        return;
    };

    for i in i_start..i_end {
        let width = zone.widths()[i];
        if width == 0 {
            continue;
        }

        // Safety: strict_sub() won't panic here because as constructed above,
        // min(i) = src_base.1 - zone_base.1
        let src_y = (zone_base.1 + i).strict_sub(src_base.1);

        let row_start = zone_base.0 + zone.offsets()[i];
        let row_end = row_start + width;

        let overlap_start = row_start.max(src_base.0);
        let overlap_end = row_end.min(src_base.0 + src_easting_size);
        if overlap_start >= overlap_end {
            continue;
        }

        // Safety: strict_sub() won't panic here because as constructed above,
        // overlap_end > overlap_start >= row_start &&
        // overlap_end > overlap_start >= src_base.0
        let j_start = overlap_start.strict_sub(row_start);
        let j_end = overlap_end.strict_sub(row_start);
        let src_x_start = overlap_start.strict_sub(src_base.0);
        let src_x_end = overlap_end.strict_sub(src_base.0);

        let src_slice = src.slice(s![src_x_start..src_x_end, src_y]);
        height_values
            .slice_mut(s![i, j_start..j_end])
            .zip_mut_with(&src_slice, |dst, &s| *dst = (*dst).max(s));
    }
}

fn bilt_obstruction(
    obstruction_meta: &ObstructionMeta,
    obstruction_raster: &ObstructionRaster,
    height_values: &mut Array2<u16>,
    zone: &FresnelZone,
) {
    let c = obstruction_meta.sw_offset();
    let src_base = (c.easting().floor() as usize, c.northing().floor() as usize);
    bilt_impl(
        src_base,
        obstruction_raster.heightmap().view(),
        height_values,
        zone,
    );
}

fn bilt_tile(tile: &ElevationTile, height_values: &mut Array2<u16>, zone: &FresnelZone) {
    let c = tile.id().get_sw_corner();
    let src_base = (c.easting().floor() as usize, c.northing().floor() as usize);
    bilt_impl(
        src_base,
        tile.elevation_inches().view(),
        height_values,
        zone,
    );
}

#[derive(new)]
pub struct TerrainFactory<'a> {
    tile_provider: &'a (dyn ElevationTileProvider + Sync + Send),
    obstruction_provider: &'a (dyn ObstructionProvider + Sync + Send),
}

impl<'a> TerrainFactory<'a> {
    pub async fn load_terrain_grid(
        &self,
        tile_ids: &HashSet<TileId>,
        zone: &FresnelZone,
        obs_filter: &ObstructionTypesFilter,
    ) -> Result<TerrainGrid, AssetErr> {
        let mut height_values = Array2::<u16>::zeros(zone.values().raw_dim());

        let tiles: Vec<ElevationTile> = stream::iter(tile_ids.iter().copied())
            .map(|id| self.tile_provider.get_elevation_tile(id))
            .buffered(PER_LOAD_TILES_CALL_CONCURRENCY_LIMIT_TILES)
            .try_collect()
            .await?;

        let all_obstruction_ids: HashSet<(ObstructionType, ObstructionId)> =
            stream::iter(tile_ids.iter().copied())
                .map(|id| self.obstruction_provider.get_obstruction_ids_for_tile(id))
                .buffered(PER_LOAD_TILES_CALL_CONCURRENCY_LIMIT_OBSTRUCTIONS)
                .try_collect::<Vec<_>>()
                .await?
                .into_iter()
                .flat_map(|map| {
                    map.into_iter()
                        .flat_map(|(t, ids)| ids.into_iter().map(move |id| (t.clone(), id)))
                })
                .filter(|(type_, _)| obs_filter.includes(type_))
                .collect();

        let obstruction_provider = self.obstruction_provider;
        let obstructions: Vec<(ObstructionMeta, ObstructionRaster)> =
            stream::iter(all_obstruction_ids)
                .map(move |(obstruction_type, obstruction_id)| async move {
                    tokio::try_join!(
                        obstruction_provider
                            .get_obstruction_meta(&obstruction_type, obstruction_id),
                        obstruction_provider
                            .get_obstruction_raster(&obstruction_type, obstruction_id),
                    )
                })
                .buffered(PER_LOAD_TILES_CALL_CONCURRENCY_LIMIT_OBSTRUCTIONS)
                .try_collect()
                .await?;

        for tile in &tiles {
            bilt_tile(tile, &mut height_values, zone);
        }

        for (obs_meta, obs_raster) in &obstructions {
            bilt_obstruction(obs_meta, obs_raster, &mut height_values, zone);
        }

        Ok(TerrainGrid::new(
            height_values,
            zone.widths().clone(),
            zone.offsets().clone(),
            zone.base_offset().clone(),
        ))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::analysis::fresnel_zone::FresnelZonePoint;
    use crate::types::errors::AssetErr;
    use async_trait::async_trait;
    use maplit::{btreeset, hashset};
    use ndarray::{Array1, Array2, array};
    use pretty_assertions::{assert_eq};
    use std::collections::{BTreeSet, HashMap, HashSet};
    use std::io::Cursor;

    // --- bilt_tile helpers ---

    fn flat_zone(
        base: NYSCoords2,
        nrows: usize,
        ncols: usize,
        width: usize,
        offset: usize,
    ) -> FresnelZone {
        FresnelZone::new(
            Array2::<FresnelZonePoint>::default((nrows, ncols)),
            Array1::from_elem(nrows, width),
            Array1::from_elem(nrows, offset),
            base,
        )
    }

    fn flat_tile(id: TileId, elevation: u16) -> ElevationTile {
        let side = SUBGRID_TILE_SIDE_LENGTH_USFT as usize;
        ElevationTile::new(id, Array2::from_elem((side, side), elevation))
    }

    // --- bilt_tile tests ---
    // Tile IDs used below and their SW corners (easting, northing):
    //   "2235_00" → (1_002_500, 235_000)
    //   "2235_01" → (1_002_500, 235_500)
    //   "2232_04" → (1_002_500, 234_500)

    #[test]
    fn bilt_tile_copies_aligned_tile_values() {
        // Tile and zone share the same SW corner; every cell should be filled.
        let id = TileId::parse("2235_00").unwrap();
        let tile = flat_tile(id, 7);
        let zone = flat_zone(NYSCoords2::new(1_002_500.0, 235_000.0), 500, 500, 500, 0);
        let mut hv = Array2::<u16>::zeros(zone.values().raw_dim());

        bilt_tile(&tile, &mut hv, &zone);

        assert!(hv.iter().all(|&v| v == 7));
    }

    #[test]
    fn bilt_tile_skips_zero_width_rows() {
        let id = TileId::parse("2235_00").unwrap();
        let tile = flat_tile(id, 7);
        let nrows = 500usize;
        // Alternating: even rows have width 500, odd rows have width 0.
        let widths = Array1::from_iter((0..nrows).map(|i| if i % 2 == 0 { 500 } else { 0 }));
        let zone = FresnelZone::new(
            Array2::<FresnelZonePoint>::default((nrows, 500)),
            widths,
            Array1::zeros(nrows),
            NYSCoords2::new(1_002_500.0, 235_000.0),
        );
        let mut hv = Array2::<u16>::zeros(zone.values().raw_dim());

        bilt_tile(&tile, &mut hv, &zone);

        for i in 0..nrows {
            let expected = if i % 2 == 0 { 7 } else { 0 };
            assert!(
                hv.row(i).iter().all(|&v| v == expected),
                "row {i}: expected all {expected}"
            );
        }
    }

    #[test]
    fn bilt_tile_no_easting_overlap_leaves_zeros() {
        // Zone covers easting [1_001_000, 1_001_500); tile starts at 1_002_500. No overlap.
        let id = TileId::parse("2235_00").unwrap();
        let tile = flat_tile(id, 42);
        let zone = flat_zone(NYSCoords2::new(1_001_000.0, 235_000.0), 500, 500, 500, 0);
        let mut hv = Array2::<u16>::zeros(zone.values().raw_dim());

        bilt_tile(&tile, &mut hv, &zone);

        assert!(hv.iter().all(|&v| v == 0));
    }

    #[test]
    fn bilt_tile_tile_inside_wider_zone_leaves_outer_columns_zero() {
        // Zone easting [1_002_000, 1_003_500), tile easting [1_002_500, 1_003_000).
        // Only columns 500..1000 of height_values should be filled.
        let id = TileId::parse("2235_00").unwrap();
        let tile = flat_tile(id, 55);
        let zone = flat_zone(NYSCoords2::new(1_002_000.0, 235_000.0), 500, 1500, 1500, 0);
        let mut hv = Array2::<u16>::zeros(zone.values().raw_dim());

        bilt_tile(&tile, &mut hv, &zone);

        for i in 0..500usize {
            let row = hv.row(i);
            assert!(
                row.iter().take(500).all(|&v| v == 0),
                "row {i}: west columns should be zero"
            );
            assert!(
                row.iter().skip(500).take(500).all(|&v| v == 55),
                "row {i}: tile columns should be filled"
            );
            assert!(
                row.iter().skip(1000).all(|&v| v == 0),
                "row {i}: east columns should be zero"
            );
        }
    }

    #[test]
    fn bilt_tile_tile_north_of_zone_base_fills_upper_rows() {
        // Tile SW northing = 235_500, zone base northing = 235_000 (tile starts 500 north).
        // Zone has 1000 rows: rows 0..500 are not covered; rows 500..1000 are.
        let id = TileId::parse("2235_01").unwrap(); // SW at (1_002_500, 235_500)
        let tile = flat_tile(id, 33);
        let zone = flat_zone(NYSCoords2::new(1_002_500.0, 235_000.0), 1000, 500, 500, 0);
        let mut hv = Array2::<u16>::zeros(zone.values().raw_dim());

        bilt_tile(&tile, &mut hv, &zone);

        for i in 0..500usize {
            assert!(
                hv.row(i).iter().all(|&v| v == 0),
                "row {i}: above tile, should be zero"
            );
        }
        for i in 500..1000usize {
            assert!(
                hv.row(i).iter().all(|&v| v == 33),
                "row {i}: within tile, should be filled"
            );
        }
    }

    #[test]
    fn bilt_tile_tile_south_of_zone_base_fills_lower_rows() {
        // Tile "2232_04": SW at (1_002_500, 234_500), covers northing [234_500, 235_000).
        // Zone base northing = 234_750, so tile overlaps zone rows 0..250
        // (tile northing rows 250..500 land inside the zone).
        let id = TileId::parse("2232_04").unwrap(); // SW at (1_002_500, 234_500)
        let tile = flat_tile(id, 99);
        let zone = flat_zone(NYSCoords2::new(1_002_500.0, 234_750.0), 500, 500, 500, 0);
        let mut hv = Array2::<u16>::zeros(zone.values().raw_dim());

        bilt_tile(&tile, &mut hv, &zone);

        for i in 0..250usize {
            assert!(
                hv.row(i).iter().all(|&v| v == 99),
                "row {i}: within overlap, should be filled"
            );
        }
        for i in 250..500usize {
            assert!(
                hv.row(i).iter().all(|&v| v == 0),
                "row {i}: beyond tile north edge, should be zero"
            );
        }
    }

    #[test]
    fn bilt_tile_maps_tile_coords_to_zone_indices_correctly() {
        // Verify the easting/northing → col/row transposition.
        // elevation_inches is [easting_local, northing_local]; height_values is [northing_row, col].
        let id = TileId::parse("2235_00").unwrap();
        let side = SUBGRID_TILE_SIDE_LENGTH_USFT as usize;
        let mut elev = Array2::<u16>::zeros((side, side));
        elev[[10, 20]] = 1000; // easting_local=10, northing_local=20
        let tile = ElevationTile::new(id, elev);
        let zone = flat_zone(NYSCoords2::new(1_002_500.0, 235_000.0), 500, 500, 500, 0);
        let mut hv = Array2::<u16>::zeros(zone.values().raw_dim());

        bilt_tile(&tile, &mut hv, &zone);

        assert_eq!(hv[[20, 10]], 1000);
        let nonzero_count: usize = hv.iter().filter(|&&v| v > 0).count();
        assert_eq!(nonzero_count, 1);
    }

    #[test]
    fn bilt_tile_respects_zone_row_offset() {
        // Zone offset=5 means each row starts 5 usft east of the zone's base easting.
        // Tile easting base = 1_002_500; row_start = 1_002_505, so tile_x_start = 5.
        // elevation_inches[[5, 0]] should appear at height_values[[0, 0]].
        let id = TileId::parse("2235_00").unwrap();
        let side = SUBGRID_TILE_SIDE_LENGTH_USFT as usize;
        let mut elev = Array2::<u16>::zeros((side, side));
        elev[[5, 0]] = 777; // easting_local=5, northing_local=0
        let tile = ElevationTile::new(id, elev);
        let zone = FresnelZone::new(
            Array2::<FresnelZonePoint>::default((500, 105)),
            Array1::from_elem(500usize, 100usize),
            Array1::from_elem(500usize, 5usize),
            NYSCoords2::new(1_002_500.0, 235_000.0),
        );
        let mut hv = Array2::<u16>::zeros(zone.values().raw_dim());

        bilt_tile(&tile, &mut hv, &zone);

        // row_start = 1_002_505, tile_x_start = 5 → j=0 maps to easting_local=5
        assert_eq!(hv[[0, 0]], 777);
    }

    fn mock_fresnel_zone(offset: NYSCoords2) -> FresnelZone {
        FresnelZone::new(
            array![
                [(1, 2), (0, 0), (0, 0), (0, 0)],
                [(1, 2), (1, 2), (1, 2), (1, 2)],
                [(1, 2), (1, 2), (1, 2), (1, 2)],
                [(1, 2), (1, 2), (0, 0), (0, 0)],
            ]
            .mapv_into_any(FresnelZonePoint::from),
            array![1, 4, 4, 2,],
            array![1, 0, 0, 1,],
            offset,
        )
    }

    #[test]
    fn test_get_intersecting_tiles_4_corner() {
        let zone = mock_fresnel_zone(NYSCoords2::new(1002498., 244998.));
        let tiles = get_intersecting_tiles(&zone);

        assert_eq!(
            tiles,
            hashset! {
                TileId::parse("242_44").unwrap(),
                TileId::parse("245_40").unwrap(),
                TileId::parse("2245_00").unwrap(),
                TileId::parse("2242_04").unwrap(),
            }
        )
    }

    #[test]
    fn test_get_intersecting_tiles_3_corner() {
        let zone = mock_fresnel_zone(NYSCoords2::new(1002497., 244997.));
        let tiles = get_intersecting_tiles(&zone);

        assert_eq!(
            tiles,
            hashset! {
                TileId::parse("242_44").unwrap(),
                TileId::parse("245_40").unwrap(),
                TileId::parse("2242_04").unwrap(),
            }
        )
    }

    #[test]
    fn test_get_intersecting_tiles_2_corner() {
        let zone = mock_fresnel_zone(NYSCoords2::new(1002496., 244997.));
        let tiles = get_intersecting_tiles(&zone);

        assert_eq!(
            tiles,
            hashset! {
                TileId::parse("242_44").unwrap(),
                TileId::parse("245_40").unwrap(),
            }
        )
    }

    #[test]
    fn test_get_intersecting_tiles_1_corner() {
        let zone = mock_fresnel_zone(NYSCoords2::new(1002496., 244996.));
        let tiles = get_intersecting_tiles(&zone);

        assert_eq!(
            tiles,
            hashset! {
                TileId::parse("242_44").unwrap(),
            }
        )
    }

    #[test]
    fn test_get_intersecting_tiles_wider_than_tile() {
        let zone = FresnelZone::new(
            Array2::<(u16, u16)>::default((2800, 4)).mapv_into_any(FresnelZonePoint::from),
            array![2798, 2800, 2800, 2798],
            array![1, 0, 0, 1,],
            NYSCoords2::new(1002490., 244990.),
        );
        let tiles = get_intersecting_tiles(&zone);

        assert_eq!(
            tiles
                .iter()
                .map(TileId::to_string)
                .collect::<BTreeSet<String>>(),
            btreeset! {
                "242_44".to_string(),
                "2242_04".to_string(),
                "2242_14".to_string(),
                "2242_24".to_string(),
                "2242_34".to_string(),
                "2242_44".to_string(),
                "5242_04".to_string(),
            }
        );
    }

    #[test]
    fn test_get_intersecting_tiles_wider_than_tile_due_to_offset() {
        let zone = FresnelZone::new(
            Array2::<(u16, u16)>::default((300, 6)).mapv_into_any(FresnelZonePoint::from),
            array![300, 300, 300, 300, 300, 300],
            array![0, 100, 200, 300, 400, 500],
            NYSCoords2::new(1002490., 244990.),
        );
        let tiles = get_intersecting_tiles(&zone);

        assert_eq!(
            tiles
                .iter()
                .map(TileId::to_string)
                .collect::<BTreeSet<String>>(),
            btreeset! {
                "242_44".to_string(),
                "2242_04".to_string(),
                "2242_14".to_string(),
            }
        );
    }

    #[test]
    fn test_get_intersecting_tiles_stairstepping_excludes_corners() {
        let zone = FresnelZone::new(
            Array2::<(u16, u16)>::default((5, 2500)).mapv_into_any(FresnelZonePoint::from),
            Array1::from_iter(std::iter::repeat_n(5, 2500)),
            Array1::from_iter((0..).step_by(1).take(2500)),
            // 1002500,245000
            NYSCoords2::new(1002500., 245000.),
        );
        let tiles = get_intersecting_tiles(&zone);

        assert_eq!(
            tiles
                .iter()
                .map(TileId::to_string)
                .collect::<BTreeSet<String>>(),
            btreeset! {
                "2245_00".to_string(),
                "2245_10".to_string(),
                "2245_11".to_string(),
                "2245_21".to_string(),
                "2245_22".to_string(),
                "2245_32".to_string(),
                "2245_33".to_string(),
                "2245_43".to_string(),
                "2245_44".to_string(),
                "5245_04".to_string(),
            }
        )
    }

    // --- bilt_obstruction helpers ---
    // Obstruction coordinates are arbitrary (not snapped to tile boundaries).
    // SW corners used below (easting, northing):
    //   obs at zone SW   → (1_002_500, 235_000)
    //   obs inside zone  → (1_002_600, 235_100)  [100 east, 100 north of zone SW]
    //   obs north half   → (1_002_500, 235_250)  [starts 250 rows in]
    //   obs south of zone → (1_002_500, 234_800) [starts 200 below zone SW]

    fn obs_meta(sw_easting: f64, sw_northing: f64) -> ObstructionMeta {
        ObstructionMeta::from_json(
            Cursor::new(
                &format!(
                    r#"{{
            "obstruction_id": "00000000-0000-0000-0000-000000000001",
            "obstruction_type": "test",
            "attributes": {{}},
            "x_offset": {sw_easting},
            "y_offset": {sw_northing},
            "tile_ids": []
        }}"#
                )
                .into_bytes(),
            ),
            ObstructionType::ActivePermits,
        )
        .unwrap()
    }

    fn flat_obstruction(
        sw_easting: f64,
        sw_northing: f64,
        easting_size: usize,
        northing_size: usize,
        value: u16,
    ) -> (ObstructionMeta, ObstructionRaster) {
        let meta = obs_meta(sw_easting, sw_northing);
        let raster =
            ObstructionRaster::new(Array2::from_elem((easting_size, northing_size), value));
        (meta, raster)
    }

    // --- bilt_obstruction tests ---

    #[test]
    fn bilt_obstruction_copies_aligned_values() {
        // Obstruction SW matches zone SW; obstruction covers the full zone footprint.
        let zone = flat_zone(NYSCoords2::new(1_002_500.0, 235_000.0), 300, 200, 200, 0);
        let (meta, raster) = flat_obstruction(1_002_500.0, 235_000.0, 200, 300, 13);
        let mut hv = Array2::<u16>::zeros(zone.values().raw_dim());

        bilt_obstruction(&meta, &raster, &mut hv, &zone);

        assert!(hv.iter().all(|&v| v == 13));
    }

    #[test]
    fn bilt_obstruction_no_easting_overlap_leaves_zeros() {
        // Obstruction is entirely east of the zone; no column overlap.
        let zone = flat_zone(NYSCoords2::new(1_002_500.0, 235_000.0), 300, 200, 200, 0);
        let (meta, raster) = flat_obstruction(1_002_700.0, 235_000.0, 100, 300, 99);
        let mut hv = Array2::<u16>::zeros(zone.values().raw_dim());

        bilt_obstruction(&meta, &raster, &mut hv, &zone);

        assert!(hv.iter().all(|&v| v == 0));
    }

    #[test]
    fn bilt_obstruction_partial_northing_overlap_north() {
        // Obstruction SW is 250 rows north of zone base; rows 0..250 are untouched.
        let zone = flat_zone(NYSCoords2::new(1_002_500.0, 235_000.0), 500, 200, 200, 0);
        let (meta, raster) = flat_obstruction(1_002_500.0, 235_250.0, 200, 400, 7);
        let mut hv = Array2::<u16>::zeros(zone.values().raw_dim());

        bilt_obstruction(&meta, &raster, &mut hv, &zone);

        for i in 0..250usize {
            assert!(
                hv.row(i).iter().all(|&v| v == 0),
                "row {i}: below obstruction, should be zero"
            );
        }
        for i in 250..500usize {
            assert!(
                hv.row(i).iter().all(|&v| v == 7),
                "row {i}: within obstruction, should be filled"
            );
        }
    }

    #[test]
    fn bilt_obstruction_partial_northing_overlap_south() {
        // Obstruction SW is 200 rows south of zone base; only rows 0..200 are covered.
        let zone = flat_zone(NYSCoords2::new(1_002_500.0, 235_000.0), 500, 200, 200, 0);
        let (meta, raster) = flat_obstruction(1_002_500.0, 234_800.0, 200, 400, 5);
        let mut hv = Array2::<u16>::zeros(zone.values().raw_dim());

        bilt_obstruction(&meta, &raster, &mut hv, &zone);

        for i in 0..200usize {
            assert!(
                hv.row(i).iter().all(|&v| v == 5),
                "row {i}: within overlap, should be filled"
            );
        }
        for i in 200..500usize {
            assert!(
                hv.row(i).iter().all(|&v| v == 0),
                "row {i}: beyond obstruction north edge, should be zero"
            );
        }
    }

    #[test]
    fn bilt_obstruction_smaller_than_zone_fills_only_footprint() {
        // Obstruction is 100 wide × 200 tall, placed 100 east and 100 north of zone SW.
        // Zone is 500 wide × 500 tall. Only rows 100..300, cols 100..200 should be filled.
        let zone = flat_zone(NYSCoords2::new(1_002_500.0, 235_000.0), 500, 500, 500, 0);
        let (meta, raster) = flat_obstruction(1_002_600.0, 235_100.0, 100, 200, 42);
        let mut hv = Array2::<u16>::zeros(zone.values().raw_dim());

        bilt_obstruction(&meta, &raster, &mut hv, &zone);

        for i in 0..500usize {
            let row = hv.row(i);
            if !(100..300).contains(&i) {
                assert!(
                    row.iter().all(|&v| v == 0),
                    "row {i}: outside northing footprint, should be zero"
                );
            } else {
                assert!(
                    row.iter().take(100).all(|&v| v == 0),
                    "row {i}: west of footprint, should be zero"
                );
                assert!(
                    row.iter().skip(100).take(100).all(|&v| v == 42),
                    "row {i}: within footprint, should be 42"
                );
                assert!(
                    row.iter().skip(200).all(|&v| v == 0),
                    "row {i}: east of footprint, should be zero"
                );
            }
        }
    }

    #[test]
    fn bilt_obstruction_shorter_wide_does_not_overwrite_taller_narrow() {
        // Bug: a short-but-wide obstruction blit'd after a tall narrow one overwrites the tall
        // values wherever their footprints overlap, because bilt_impl uses assign rather than
        // element-wise max. This produces a false negative: the Fresnel zone clears the short
        // building but would be blocked by the tall one — except the tall one's height was
        // silently replaced.
        //
        // Setup: tall building (10 000 in, ~833 ft) at (1_002_550, 235_050), 50×50 usft.
        //        short building ( 100 in,   ~8 ft) at (1_002_500, 235_000), 200×200 usft.
        // The short building's footprint fully contains the tall one.
        // After blitting tall-first then short, the tall values must survive at their footprint.
        let zone = flat_zone(NYSCoords2::new(1_002_500.0, 235_000.0), 300, 300, 300, 0);

        // Tall narrow obstruction: zone rows 50..100, cols 50..100
        let (meta_tall, raster_tall) = flat_obstruction(1_002_550.0, 235_050.0, 50, 50, 10_000);
        // Short wide obstruction: zone rows 0..200, cols 0..200 — entirely contains the tall one
        let (meta_short, raster_short) = flat_obstruction(1_002_500.0, 235_000.0, 200, 200, 100);

        let mut hv = Array2::<u16>::zeros(zone.values().raw_dim());
        bilt_obstruction(&meta_tall, &raster_tall, &mut hv, &zone);
        bilt_obstruction(&meta_short, &raster_short, &mut hv, &zone);

        // The tall building's footprint (zone rows 50..100, cols 50..100) must still show 10 000.
        let tall_region_max = hv.slice(s![50..100, 50..100]).iter().copied().max().unwrap_or(0);
        assert_eq!(
            tall_region_max, 10_000,
            "tall obstruction height must not be overwritten by the shorter surrounding one"
        );
    }

    #[test]
    fn bilt_obstruction_maps_coords_correctly() {
        // A single lit pixel at easting_local=30, northing_local=50 in the obstruction raster
        // should appear at height_values[[50, 30]] (row=northing, col=easting).
        let zone = flat_zone(NYSCoords2::new(1_002_500.0, 235_000.0), 200, 200, 200, 0);
        let meta = obs_meta(1_002_500.0, 235_000.0);
        let mut heightmap = Array2::<u16>::zeros((200, 200));
        heightmap[[30, 50]] = 888; // easting_local=30, northing_local=50
        let raster = ObstructionRaster::new(heightmap);
        let mut hv = Array2::<u16>::zeros(zone.values().raw_dim());

        bilt_obstruction(&meta, &raster, &mut hv, &zone);

        assert_eq!(hv[[50, 30]], 888);
        assert_eq!(hv.iter().filter(|&&v| v > 0).count(), 1);
    }

    // --- load_terrain_grid helpers ---

    struct EmptyObstructionProvider;

    #[async_trait]
    impl ObstructionProvider for EmptyObstructionProvider {
        async fn get_obstruction_ids_for_tile(
            &self,
            _tile_id: TileId,
        ) -> Result<HashMap<ObstructionType, Vec<ObstructionId>>, AssetErr> {
            Ok(HashMap::new())
        }
        async fn get_obstruction_meta(
            &self,
            _t: &ObstructionType,
            _id: ObstructionId,
        ) -> Result<ObstructionMeta, AssetErr> {
            unreachable!()
        }
        async fn get_obstruction_raster(
            &self,
            _t: &ObstructionType,
            _id: ObstructionId,
        ) -> Result<ObstructionRaster, AssetErr> {
            unreachable!()
        }
    }

    struct MockTileProvider {
        tiles: HashMap<TileId, u16>,
        fail: bool,
    }

    #[async_trait]
    impl ElevationTileProvider for MockTileProvider {
        async fn get_elevation_tile(&self, tile_id: TileId) -> Result<ElevationTile, AssetErr> {
            if self.fail {
                return Err(AssetErr::AssetNotFound("mock failure".into()));
            }
            let elevation = *self.tiles.get(&tile_id).unwrap_or(&0);
            Ok(flat_tile(tile_id, elevation))
        }
    }

    // --- load_terrain_grid tests ---

    #[tokio::test]
    async fn load_terrain_grid_propagates_provider_error() {
        let provider = MockTileProvider {
            tiles: HashMap::new(),
            fail: true,
        };
        let factory = TerrainFactory::new(&provider, &EmptyObstructionProvider);
        let zone = flat_zone(NYSCoords2::new(1_002_500.0, 235_000.0), 500, 500, 500, 0);
        let tile_ids = hashset! { TileId::parse("2235_00").unwrap() };

        let result = factory
            .load_terrain_grid(&tile_ids, &zone, &ObstructionTypesFilter::All)
            .await;

        assert!(matches!(result, Err(AssetErr::AssetNotFound(_))));
    }

    #[tokio::test]
    async fn load_terrain_grid_empty_tile_set_returns_zero_grid_with_zone_metadata() {
        let provider = MockTileProvider {
            tiles: HashMap::new(),
            fail: false,
        };
        let factory = TerrainFactory::new(&provider, &EmptyObstructionProvider);
        let base = NYSCoords2::new(1_002_500.0, 235_000.0);
        let zone = flat_zone(base.clone(), 500, 300, 300, 5);

        let result = factory
            .load_terrain_grid(&HashSet::new(), &zone, &ObstructionTypesFilter::All)
            .await
            .unwrap();

        assert!(result.values().iter().all(|&v| v == 0));
        assert_eq!(result.widths(), zone.widths());
        assert_eq!(result.offsets(), zone.offsets());
        assert_eq!(result.base_offset(), zone.base_offset());
    }

    #[tokio::test]
    async fn load_terrain_grid_accumulates_two_non_overlapping_tiles() {
        // "2235_00": SW at (1_002_500, 235_000), covers easting [1_002_500, 1_003_000)
        // "2235_10": SW at (1_003_000, 235_000), covers easting [1_003_000, 1_003_500)
        let id_west = TileId::parse("2235_00").unwrap();
        let id_east = TileId::parse("2235_10").unwrap();
        let provider = MockTileProvider {
            tiles: HashMap::from([(id_west, 11u16), (id_east, 22u16)]),
            fail: false,
        };
        let factory = TerrainFactory::new(&provider, &EmptyObstructionProvider);
        // Zone spans both tiles: easting [1_002_500, 1_003_500)
        let zone = flat_zone(NYSCoords2::new(1_002_500.0, 235_000.0), 500, 1000, 1000, 0);
        let tile_ids = hashset! { id_west, id_east };

        let result = factory
            .load_terrain_grid(&tile_ids, &zone, &ObstructionTypesFilter::All)
            .await
            .unwrap();

        for i in 0..500usize {
            let row = result.values().row(i);
            assert!(
                row.iter().take(500).all(|&v| v == 11),
                "row {i}: west region should be 11"
            );
            assert!(
                row.iter().skip(500).all(|&v| v == 22),
                "row {i}: east region should be 22"
            );
        }
    }

    #[test]
    fn bilt_tile_tile_extends_north_beyond_zone_does_not_panic() {
        let id = TileId::parse("2235_01").unwrap(); // SW at (1_002_500, 235_500)
        let tile = flat_tile(id, 9);
        let zone = flat_zone(NYSCoords2::new(1_002_500.0, 235_000.0), 600, 500, 500, 0);
        let mut hv = Array2::<u16>::zeros(zone.values().raw_dim());

        bilt_tile(&tile, &mut hv, &zone); // would panic with the old i_end formula

        for i in 0..500usize {
            assert!(
                hv.row(i).iter().all(|&v| v == 0),
                "row {i}: outside tile, should be zero"
            );
        }
        for i in 500..600usize {
            assert!(
                hv.row(i).iter().all(|&v| v == 9),
                "row {i}: within overlap, should be filled"
            );
        }
    }

    // --- MockObstructionProvider ---

    struct MockObstructionProvider {
        tile_id: TileId,
        obs_id: ObstructionId,
        sw_easting: f64,
        sw_northing: f64,
        raster_easting: usize,
        raster_northing: usize,
        raster_value: u16,
    }

    #[async_trait]
    impl ObstructionProvider for MockObstructionProvider {
        async fn get_obstruction_ids_for_tile(
            &self,
            tile_id: TileId,
        ) -> Result<HashMap<ObstructionType, Vec<ObstructionId>>, AssetErr> {
            if tile_id == self.tile_id {
                Ok(HashMap::from([(
                    ObstructionType::ActivePermits,
                    vec![self.obs_id],
                )]))
            } else {
                Ok(HashMap::new())
            }
        }
        async fn get_obstruction_meta(
            &self,
            _t: &ObstructionType,
            _id: ObstructionId,
        ) -> Result<ObstructionMeta, AssetErr> {
            Ok(obs_meta(self.sw_easting, self.sw_northing))
        }
        async fn get_obstruction_raster(
            &self,
            _t: &ObstructionType,
            _id: ObstructionId,
        ) -> Result<ObstructionRaster, AssetErr> {
            Ok(ObstructionRaster::new(Array2::from_elem(
                (self.raster_easting, self.raster_northing),
                self.raster_value,
            )))
        }
    }

    #[tokio::test]
    async fn load_terrain_grid_obstruction_overwrites_terrain_in_footprint() {
        // Tile "2235_00" fills the whole zone at elevation 10.
        // A 100×200 obstruction placed at (1_002_600, 235_100) then overwrites that region.
        // Expected: rows 100..300, cols 100..200 → 5000; everything else → 10.
        let tile_id = TileId::parse("2235_00").unwrap();
        let tile_provider = MockTileProvider {
            tiles: HashMap::from([(tile_id, 10u16)]),
            fail: false,
        };
        let obs_provider = MockObstructionProvider {
            tile_id,
            obs_id: uuid::Uuid::nil(),
            sw_easting: 1_002_600.0,
            sw_northing: 235_100.0,
            raster_easting: 100,
            raster_northing: 200,
            raster_value: 5000,
        };
        let factory = TerrainFactory::new(&tile_provider, &obs_provider);
        let zone = flat_zone(NYSCoords2::new(1_002_500.0, 235_000.0), 500, 500, 500, 0);

        let result = factory
            .load_terrain_grid(&hashset! { tile_id }, &zone, &ObstructionTypesFilter::All)
            .await
            .unwrap();

        for i in 0..500usize {
            let row = result.values().row(i);
            if !(100..300).contains(&i) {
                assert!(
                    row.iter().all(|&v| v == 10),
                    "row {i}: outside obstruction northing, should be terrain"
                );
            } else {
                assert!(
                    row.iter().take(100).all(|&v| v == 10),
                    "row {i}: west of obstruction, should be terrain"
                );
                assert!(
                    row.iter().skip(100).take(100).all(|&v| v == 5000),
                    "row {i}: obstruction footprint"
                );
                assert!(
                    row.iter().skip(200).all(|&v| v == 10),
                    "row {i}: east of obstruction, should be terrain"
                );
            }
        }
    }
}
