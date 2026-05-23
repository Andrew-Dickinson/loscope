use std::collections::HashSet;
use std::hash::Hash;
use std::isize;
use std::ops::Sub;
use derive_getters::Getters;
use derive_new::new;
use geo::Convert;
use ndarray::{s, Array1, Array2};
use crate::analysis::fresnel_zone::FresnelZone;
use crate::providers::elevation_tile_provider::{ElevationTile, ElevationTileProvider};
use crate::types::coords::NYSCoords2;
use crate::types::errors::AssetErr;
use crate::types::stairstep::StairStepGrid;
use crate::types::tiles::{TileId, SUBGRID_TILE_SIDE_LENGTH_USFT};


pub(crate) type TerrainGrid = StairStepGrid<u16>;

pub fn get_intersecting_tiles(fresnel_zone: &FresnelZone) -> HashSet<TileId> {
    let max_width = (fresnel_zone.values().ncols() as f64 / f64::from(SUBGRID_TILE_SIDE_LENGTH_USFT)).ceil() as usize + 1;
    let max_height = (fresnel_zone.values().nrows() as f64 / f64::from(SUBGRID_TILE_SIDE_LENGTH_USFT)).ceil() as usize + 1;
    let max_tiles_count = max_width * max_height;

    let mut intersecting_tiles = HashSet::with_capacity(max_tiles_count);

    fresnel_zone.widths().iter().zip(fresnel_zone.offsets())
        .enumerate()
        .for_each(|(i, (width, offset))| {
            let sample_point = NYSCoords2::new(
                fresnel_zone.base_offset().easting() + *offset as f64,
                fresnel_zone.base_offset().northing() + i as f64
            );
            let west_tile = TileId::from_contained_point(&sample_point);
            let west_tile_e_edge = *west_tile.get_sw_corner().easting() + SUBGRID_TILE_SIDE_LENGTH_USFT as f64;

            let row_e_edge = *sample_point.easting() + *width as f64;
            let remainder = row_e_edge - west_tile_e_edge;
            let mut tile_count_offset = 0;
            loop {
                let tile_id = TileId::from_adjacent_tile(&west_tile, (tile_count_offset, 0));
                intersecting_tiles.insert(tile_id);
                if tile_count_offset as f64 * SUBGRID_TILE_SIDE_LENGTH_USFT as f64 >= remainder { break }
                tile_count_offset += 1;
            }
        });

    intersecting_tiles
}

fn bilt_tile(tile: &ElevationTile, height_values: &mut Array2<u16>, zone: &FresnelZone) {
    let zone_base_offset = zone.base_offset();
    let tile_base_offset  = tile.id().get_sw_corner();

    let zone_base_offset = (zone_base_offset.easting().floor() as usize, zone_base_offset.northing().floor() as usize);
    let tile_base_offset = (tile_base_offset.easting().floor() as usize, tile_base_offset.northing().floor() as usize);

    let i_start = (tile_base_offset.1 as isize - zone_base_offset.1 as isize).max(0) as usize;
    let i_end: usize = ((tile_base_offset.1 as isize - zone_base_offset.1 as isize).min(zone.widths().len() as isize)
        + SUBGRID_TILE_SIDE_LENGTH_USFT as isize)
        .try_into()
        .expect("Tile selection logic issue, tile must have at least one pixel NE of the zone's SW corner");

    for i in i_start..i_end {
        let width = zone.widths()[i];
        if width == 0 { continue; }

        // Safety: strict_sub() won't panic here because as constructed above,
        // min(i) = tile_base_offset.1 - zone_base_offset.1
        let tile_y = (zone_base_offset.1 + i).strict_sub(tile_base_offset.1);

        let row_start = zone_base_offset.0 + zone.offsets()[i];
        let row_end = row_start + width;

        let overlap_start = row_start.max(tile_base_offset.0);
        let overlap_end = row_end.min(tile_base_offset.0 + usize::from(SUBGRID_TILE_SIDE_LENGTH_USFT));
        if overlap_start >= overlap_end { continue; }

        // Safety: strict_sub() won't panic here because as constructed above,
        // overlap_end > overlap_start >= row_start &&
        // overlap_end > overlap_start >= tile_base_offset.0
        let j_start = overlap_start.strict_sub(row_start);
        let j_end = overlap_end.strict_sub(row_start);
        let tile_x_start = overlap_start.strict_sub(tile_base_offset.0);
        let tile_x_end = overlap_end.strict_sub(tile_base_offset.0);

        height_values.slice_mut(s![i, j_start..j_end])
            .assign(&*tile.elevation_inches().slice(s![tile_x_start..tile_x_end, tile_y]));
    }
}

#[derive(new)]
pub struct TerrainFactory<'a> {
    tile_provider: &'a (dyn ElevationTileProvider + Sync + Send)
}

impl<'a> TerrainFactory<'a> {
    pub async fn load_terrain_grid(&self, tile_ids: &HashSet<TileId>, zone: &FresnelZone) -> Result<TerrainGrid, AssetErr> {

        let mut height_values = Array2::<u16>::zeros(zone.values().raw_dim());

        for tile_id in tile_ids {
            let tile = self.tile_provider.get_elevation_tile(*tile_id).await?;
            bilt_tile(&tile, &mut height_values, zone);

            // TODO: Compute obstructions and apply them also
        }

        Ok(
            TerrainGrid::new(
                height_values,
                zone.widths().clone(),
                zone.offsets().clone(),
                zone.base_offset().clone()
            )
        )
    }
}

#[cfg(test)]
mod test {
     use std::collections::{BTreeSet, HashMap, HashSet};
    use std::iter::repeat;
    use async_trait::async_trait;
    use maplit::{btreeset, hashset};
    use ndarray::{array, Array1, Array2};
    use crate::analysis::fresnel_zone::FresnelZonePoint;
    use crate::types::errors::AssetErr;
    use super::*;
    use pretty_assertions::{assert_eq, assert_ne};

    // --- bilt_tile helpers ---

    fn flat_zone(base: NYSCoords2, nrows: usize, ncols: usize, width: usize, offset: usize) -> FresnelZone {
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
            assert!(hv.row(i).iter().all(|&v| v == expected), "row {i}: expected all {expected}");
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
            assert!(row.iter().take(500).all(|&v| v == 0),   "row {i}: west columns should be zero");
            assert!(row.iter().skip(500).take(500).all(|&v| v == 55), "row {i}: tile columns should be filled");
            assert!(row.iter().skip(1000).all(|&v| v == 0),  "row {i}: east columns should be zero");
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
            assert!(hv.row(i).iter().all(|&v| v == 0),  "row {i}: above tile, should be zero");
        }
        for i in 500..1000usize {
            assert!(hv.row(i).iter().all(|&v| v == 33), "row {i}: within tile, should be filled");
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
            assert!(hv.row(i).iter().all(|&v| v == 99), "row {i}: within overlap, should be filled");
        }
        for i in 250..500usize {
            assert!(hv.row(i).iter().all(|&v| v == 0),  "row {i}: beyond tile north edge, should be zero");
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
                [(1, 2),(0, 0),(0, 0),(0, 0)],
                [(1, 2),(1, 2),(1, 2),(1, 2)],
                [(1, 2),(1, 2),(1, 2),(1, 2)],
                [(1, 2),(1, 2),(0, 0),(0, 0)],
            ].mapv_into_any(FresnelZonePoint::from),
            array![
                1,4,4,2,
            ],
            array![
                1,0,0,1,
            ],
            offset
        )
    }


    #[test]
    fn test_get_intersecting_tiles_4_corner() {
        let zone = mock_fresnel_zone(NYSCoords2::new(1002498.,244998.));
        let tiles = get_intersecting_tiles(&zone);

        assert_eq!(tiles, hashset!{
            TileId::parse("242_44").unwrap(),
            TileId::parse("245_40").unwrap(),
            TileId::parse("2245_00").unwrap(),
            TileId::parse("2242_04").unwrap(),
        })
    }

    #[test]
    fn test_get_intersecting_tiles_3_corner() {
        let zone = mock_fresnel_zone(NYSCoords2::new(1002497.,244997.));
        let tiles = get_intersecting_tiles(&zone);

        assert_eq!(tiles, hashset!{
            TileId::parse("242_44").unwrap(),
            TileId::parse("245_40").unwrap(),
            TileId::parse("2242_04").unwrap(),
        })
    }

    #[test]
    fn test_get_intersecting_tiles_2_corner() {
        let zone = mock_fresnel_zone(NYSCoords2::new(1002496.,244997.));
        let tiles = get_intersecting_tiles(&zone);

        assert_eq!(tiles, hashset!{
            TileId::parse("242_44").unwrap(),
            TileId::parse("245_40").unwrap(),
        })
    }

    #[test]
    fn test_get_intersecting_tiles_1_corner() {
        let zone = mock_fresnel_zone(NYSCoords2::new(1002496.,244996.));
        let tiles = get_intersecting_tiles(&zone);

        assert_eq!(tiles, hashset!{
            TileId::parse("242_44").unwrap(),
        })
    }

    #[test]
    fn test_get_intersecting_tiles_wider_than_tile() {
        let zone = FresnelZone::new(
            Array2::<(u16,u16)>::default((2800, 4)).mapv_into_any(FresnelZonePoint::from),
            array![
                2798,2800,2800,2798
            ],
            array![
                1,0,0,1,
            ],
            NYSCoords2::new(1002490.,244990.)
        );
        let tiles = get_intersecting_tiles(&zone);

        assert_eq!(tiles.iter().map(TileId::to_string).collect::<BTreeSet<String>>(),
                   btreeset!{
            "242_44".to_string(),
            "2242_04".to_string(),
            "2242_14".to_string(),
            "2242_24".to_string(),
            "2242_34".to_string(),
            "2242_44".to_string(),
            "5242_04".to_string(),
        });
    }

    #[test]
    fn test_get_intersecting_tiles_wider_than_tile_due_to_offset() {
        let zone = FresnelZone::new(
            Array2::<(u16,u16)>::default((300, 6)).mapv_into_any(FresnelZonePoint::from),
            array![
                300,300,300,300,300,300
            ],
            array![
                0,100,200,300,400,500
            ],
            NYSCoords2::new(1002490.,244990.)
        );
        let tiles = get_intersecting_tiles(&zone);

        assert_eq!(tiles.iter().map(TileId::to_string).collect::<BTreeSet<String>>(),
           btreeset!{
            "242_44".to_string(),
            "2242_04".to_string(),
            "2242_14".to_string(),
        });
    }

    #[test]
    fn test_get_intersecting_tiles_stairstepping_excludes_corners() {
        let zone = FresnelZone::new(
            Array2::<(u16,u16)>::default((5, 2500)).mapv_into_any(FresnelZonePoint::from),
            Array1::from_iter(repeat(5).take(2500)),
            Array1::from_iter((0..).step_by(1).take(2500)),
            // 1002500,245000
            NYSCoords2::new(1002500.,245000.)
        );
        let tiles = get_intersecting_tiles(&zone);

        assert_eq!(tiles.iter().map(TileId::to_string).collect::<BTreeSet<String>>(),
           btreeset!{
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
        })
    }

    // --- load_terrain_grid helpers ---

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
        let provider = MockTileProvider { tiles: HashMap::new(), fail: true };
        let factory = TerrainFactory::new(&provider);
        let zone = flat_zone(NYSCoords2::new(1_002_500.0, 235_000.0), 500, 500, 500, 0);
        let tile_ids = hashset! { TileId::parse("2235_00").unwrap() };

        let result = factory.load_terrain_grid(&tile_ids, &zone).await;

        assert!(matches!(result, Err(AssetErr::AssetNotFound(_))));
    }

    #[tokio::test]
    async fn load_terrain_grid_empty_tile_set_returns_zero_grid_with_zone_metadata() {
        let provider = MockTileProvider { tiles: HashMap::new(), fail: false };
        let factory = TerrainFactory::new(&provider);
        let base = NYSCoords2::new(1_002_500.0, 235_000.0);
        let zone = flat_zone(base.clone(), 500, 300, 300, 5);

        let result = factory.load_terrain_grid(&HashSet::new(), &zone).await.unwrap();

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
        let factory = TerrainFactory::new(&provider);
        // Zone spans both tiles: easting [1_002_500, 1_003_500)
        let zone = flat_zone(NYSCoords2::new(1_002_500.0, 235_000.0), 500, 1000, 1000, 0);
        let tile_ids = hashset! { id_west, id_east };

        let result = factory.load_terrain_grid(&tile_ids, &zone).await.unwrap();

        for i in 0..500usize {
            let row = result.values().row(i);
            assert!(row.iter().take(500).all(|&v| v == 11), "row {i}: west region should be 11");
            assert!(row.iter().skip(500).all(|&v| v == 22), "row {i}: east region should be 22");
        }
    }
}