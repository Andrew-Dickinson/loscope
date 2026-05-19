use std::collections::HashSet;
use std::hash::Hash;
use derive_getters::Getters;
use derive_new::new;
use geo::Convert;
use crate::analysis::fresnel_zone::FresnelZone;
use crate::providers::elevation_tile_provider::ElevationTileProvider;
use crate::types::coords::NYSCoords2;
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

#[derive(new)]
pub struct TerrainFactory<'a> {
    tile_provider: &'a (dyn ElevationTileProvider + Sync + Send)
}

impl<'a> TerrainFactory<'a> {
    pub fn load_terrain_grid(&self, tile_ids: &HashSet<TileId>, zone: &FresnelZone) -> TerrainGrid {
        // self.tile_provider.get_elevation_tile() ...
        todo!()
    }
}

#[cfg(test)]
mod test {
    use std::collections::BTreeSet;
    use std::iter::repeat;
    use maplit::{btreeset, hashset};
    use ndarray::{array, Array1, Array2};
    use crate::analysis::fresnel_zone::FresnelZonePoint;
    use super::*;
    use pretty_assertions::{assert_eq, assert_ne};

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
}