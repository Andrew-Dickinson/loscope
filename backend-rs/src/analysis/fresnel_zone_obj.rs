use crate::analysis::fresnel_zone::FresnelZone;
use crate::types::tiles::TileId;
use async_fn_stream::fn_stream;
use futures_util::Stream;
use ndarray::Axis;
use std::fmt::Write;
use uuid::Uuid;

pub fn stream_fresnel_tile_slice_as_obj(
    analysis_id: Uuid,
    fresnel_zone: &FresnelZone,
    tile_id: TileId,
) -> impl Stream<Item = String> {
    let fresnel_raster = fresnel_zone
        .rasterize_in_tile(tile_id)
        .mapv(|opt| opt.copied());
    // Runs during response streaming, after get_fresnel_slice_obj's memory_paranoid::scope() has
    // already returned -- not covered by a tracked reservation, so this only ever produces a
    // one-time coverage-gap warning, never a panic. (rasterize_in_tile()'s own allocation is
    // separately checked inside StairStepGrid::rasterize_in_tile.)
    crate::analysis::memory_paranoid::check(
        "stream_fresnel_tile_slice_as_obj::fresnel_raster_mapv",
        fresnel_raster.len() as u64
            * std::mem::size_of::<Option<crate::analysis::fresnel_zone::FresnelZonePoint>>() as u64,
    );

    fn_stream(|e| async move {
        e.emit(format!(
            "# Fresnel zone slice\n\
             # Analysis id: {analysis_id}\n\
             # Tile Id: {tile_id}\n\
             # X = easting (within tile), Y = northing (within tile), Z = elevation (ft)\n\
             o fresnel_zone\n\n"
        ))
        .await;

        let mut vi: usize = 0;
        let mut buf = String::with_capacity(16 * 1024);

        for (xi, col) in fresnel_raster.axis_iter(Axis(0)).enumerate() {
            for (yi, maybe_zone_point) in col.iter().enumerate() {
                let &Some(zone_point) = maybe_zone_point else {
                    continue;
                };
                let local_top = f64::from(zone_point.top()) / 12.0;
                let local_bot = f64::from(zone_point.bottom()) / 12.0;
                let (x1, y1) = (xi + 1, yi + 1);

                // Top horizontal face (normal winding)
                let v = vi + 1;
                vi += 4;
                let _ = write!(
                    buf,
                    "v {xi} {yi} {local_top:.3}\nv {x1} {yi} {local_top:.3}\n\
                     v {x1} {y1} {local_top:.3}\nv {xi} {y1} {local_top:.3}\n\
                     f {v} {} {} {}\n",
                    v + 1,
                    v + 2,
                    v + 3
                );

                // Bottom horizontal face (reversed winding — faces downward)
                let v = vi + 1;
                vi += 4;
                let _ = write!(
                    buf,
                    "v {xi} {y1} {local_bot:.3}\nv {x1} {y1} {local_bot:.3}\n\
                     v {x1} {yi} {local_bot:.3}\nv {xi} {yi} {local_bot:.3}\n\
                     f {v} {} {} {}\n",
                    v + 1,
                    v + 2,
                    v + 3
                );

                // Side faces
                for (dxi, dyi, ax, ay, bx, by) in [
                    (0isize, -1isize, xi, yi, x1, yi),
                    (0, 1, x1, y1, xi, y1),
                    (1, 0, x1, yi, x1, y1),
                    (-1, 0, xi, y1, xi, yi),
                ] {
                    let maybe_adj = xi
                        .checked_add_signed(dxi)
                        .zip(yi.checked_add_signed(dyi))
                        .and_then(|(ax_i, ay_i)| {
                            fresnel_raster.get([ax_i, ay_i]).copied().flatten()
                        });

                    match maybe_adj {
                        Some(adj_point) => {
                            let adj_top = f64::from(adj_point.top()) / 12.0;
                            let adj_bot = f64::from(adj_point.bottom()) / 12.0;
                            // Draw where the neighbour protrudes above/below this cell.
                            if adj_top > local_top {
                                let v = vi + 1;
                                vi += 4;
                                let _ = write!(
                                    buf,
                                    "v {ax} {ay} {local_top:.3}\nv {bx} {by} {local_top:.3}\n\
                                     v {bx} {by} {adj_top:.3}\nv {ax} {ay} {adj_top:.3}\n\
                                     f {v} {} {} {}\n",
                                    v + 1,
                                    v + 2,
                                    v + 3
                                );
                            }
                            if adj_bot > local_bot {
                                let v = vi + 1;
                                vi += 4;
                                let _ = write!(
                                    buf,
                                    "v {ax} {ay} {local_bot:.3}\nv {bx} {by} {local_bot:.3}\n\
                                     v {bx} {by} {adj_bot:.3}\nv {ax} {ay} {adj_bot:.3}\n\
                                     f {v} {} {} {}\n",
                                    v + 1,
                                    v + 2,
                                    v + 3
                                );
                            }
                        }
                        None => {
                            // Exposed edge: full side from bottom to top
                            let v = vi + 1;
                            vi += 4;
                            let _ = write!(
                                buf,
                                "v {ax} {ay} {local_bot:.3}\nv {bx} {by} {local_bot:.3}\n\
                                 v {bx} {by} {local_top:.3}\nv {ax} {ay} {local_top:.3}\n\
                                 f {v} {} {} {}\n",
                                v + 1,
                                v + 2,
                                v + 3
                            );
                        }
                    }
                }
            }

            if buf.len() >= 16 * 1024 {
                e.emit(std::mem::take(&mut buf)).await;
            }
        }

        if !buf.is_empty() {
            e.emit(buf).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use crate::analysis::fresnel_zone::{FresnelZone, FresnelZonePoint};
    use crate::analysis::fresnel_zone_obj::stream_fresnel_tile_slice_as_obj;
    use crate::types::coords::NYSCoords2;
    use crate::types::tiles::TileId;
    use futures_util::StreamExt;
    use ndarray::{Array1, Array2};
    use uuid::Uuid;

    // TileId "500300_00" has SW corner (500000, 300000)
    fn tile() -> TileId {
        TileId::parse("500300_00").unwrap()
    }
    fn tile_sw() -> (f64, f64) {
        (500_000.0, 300_000.0)
    }

    fn make_zone(
        values: Vec<FresnelZonePoint>,
        widths: Vec<usize>,
        offsets: Vec<usize>,
        base: (f64, f64),
    ) -> FresnelZone {
        let ncols = widths.iter().copied().max().unwrap_or(0).max(1);
        let nrows = widths.len().max(1);
        let mut padded = values;
        padded.resize(nrows * ncols, FresnelZonePoint::new(0, 0));
        FresnelZone::new(
            Array2::from_shape_vec((nrows, ncols), padded).unwrap(),
            Array1::from_vec(widths),
            Array1::from_vec(offsets),
            NYSCoords2::new(base.0, base.1),
        )
    }

    fn empty_zone() -> FresnelZone {
        FresnelZone::new(
            Array2::from_shape_vec((1, 1), vec![FresnelZonePoint::new(0, 0)]).unwrap(),
            Array1::from_vec(vec![0usize]), // width=0 → no content
            Array1::from_vec(vec![0usize]),
            NYSCoords2::new(tile_sw().0, tile_sw().1),
        )
    }

    async fn collect_zone_obj(id: Uuid, zone: &FresnelZone, tile: TileId) -> String {
        stream_fresnel_tile_slice_as_obj(id, zone, tile)
            .fold(String::new(), |mut s, chunk| async move {
                s.push_str(&chunk);
                s
            })
            .await
    }

    fn count_lines_starting_with(obj: &str, prefix: &str) -> usize {
        obj.lines().filter(|l| l.starts_with(prefix)).count()
    }

    #[tokio::test]
    async fn header_contains_analysis_id_and_tile_id() {
        let id = Uuid::new_v4();
        let obj = collect_zone_obj(id, &empty_zone(), tile()).await;
        assert!(
            obj.contains(&id.to_string()),
            "analysis id missing from header"
        );
        assert!(
            obj.contains(&tile().to_string()),
            "tile id missing from header"
        );
        assert!(obj.contains("# Fresnel zone slice"));
        assert!(obj.contains("o fresnel_zone"));
    }

    #[tokio::test]
    async fn empty_zone_produces_no_geometry() {
        let id = Uuid::new_v4();
        let obj = collect_zone_obj(id, &empty_zone(), tile()).await;
        assert_eq!(
            count_lines_starting_with(&obj, "v "),
            0,
            "no vertices for empty zone"
        );
        assert_eq!(
            count_lines_starting_with(&obj, "f "),
            0,
            "no faces for empty zone"
        );
    }

    #[tokio::test]
    async fn single_point_with_no_adjacent_produces_two_horiz_and_four_vert_faces() {
        let id = Uuid::new_v4();
        // bottom=120 in (10 ft), top=240 in (20 ft)
        let zone = make_zone(
            vec![FresnelZonePoint::new(120, 240)],
            vec![1],
            vec![0],
            tile_sw(),
        );
        let obj = collect_zone_obj(id, &zone, tile()).await;

        // 2 horizontal + 4 side (all edges exposed, None → write side face) = 6 faces total
        assert_eq!(count_lines_starting_with(&obj, "f "), 6);
        // 6 faces × 4 vertices each = 24 vertex lines
        assert_eq!(count_lines_starting_with(&obj, "v "), 24);
    }

    #[tokio::test]
    async fn single_point_horizontal_face_coords_are_correct() {
        let id = Uuid::new_v4();
        let zone = make_zone(
            vec![FresnelZonePoint::new(0, 240)], // top = 20 ft, bot = 0 ft
            vec![1],
            vec![0],
            tile_sw(),
        );
        let obj = collect_zone_obj(id, &zone, tile()).await;
        let vertices: Vec<&str> = obj.lines().filter(|l| l.starts_with("v ")).collect();

        // Top face at z=20.000 should have all four corners of [0,1]×[0,1]
        assert!(
            vertices.contains(&"v 0 0 20.000"),
            "top corner missing; got {:?}",
            &vertices[..4]
        );
        assert!(vertices.contains(&"v 1 0 20.000"));
        assert!(vertices.contains(&"v 1 1 20.000"));
        assert!(vertices.contains(&"v 0 1 20.000"));

        // Bottom face at z=0.000
        assert!(vertices.contains(&"v 0 0 0.000"));
    }

    #[tokio::test]
    async fn two_adjacent_points_share_no_exposed_vertical_face_between_them() {
        let id = Uuid::new_v4();
        // Two horizontally adjacent points; neither's inner edge is exposed
        let zone = make_zone(
            vec![
                FresnelZonePoint::new(120, 240),
                FresnelZonePoint::new(120, 240),
            ],
            vec![2],
            vec![0],
            tile_sw(),
        );
        let obj = collect_zone_obj(id, &zone, tile()).await;

        // Each point has 2 horiz + 3 exposed outer side faces. The shared inner edge is suppressed
        // because adj_top == local_top and adj_bot == local_bot (strict > check, not >=).
        // Outer exposed sides: each point has 3 (the two end sides + the far side); the shared edge
        // contributes 0 faces. Total = 2*(2+3) = 10 faces.
        let f_count = count_lines_starting_with(&obj, "f ");
        assert_eq!(
            f_count, 10,
            "each point: 2 horiz + 3 outer sides; inner shared edge suppressed"
        );
    }

    #[tokio::test]
    async fn zone_outside_tile_produces_no_geometry() {
        let id = Uuid::new_v4();
        // Base 200_000 northing is far south of tile (300_000) → no overlap
        let zone = make_zone(
            vec![FresnelZonePoint::new(120, 240)],
            vec![1],
            vec![0],
            (500_000.0, 200_000.0),
        );
        let obj = collect_zone_obj(id, &zone, tile()).await;
        assert_eq!(count_lines_starting_with(&obj, "v "), 0);
        assert_eq!(count_lines_starting_with(&obj, "f "), 0);
    }

    #[tokio::test]
    async fn face_indices_are_sequential_from_one() {
        let id = Uuid::new_v4();
        let zone = make_zone(
            vec![FresnelZonePoint::new(60, 120)], // small but nonzero
            vec![1],
            vec![0],
            tile_sw(),
        );
        let obj = collect_zone_obj(id, &zone, tile()).await;
        let face_lines: Vec<&str> = obj.lines().filter(|l| l.starts_with("f ")).collect();

        // First face references vertices 1-4
        assert!(
            face_lines[0].contains("1"),
            "first face should reference vertex 1"
        );
        // All vertex indices in face lines should be positive integers
        for face_line in &face_lines {
            let parts: Vec<&str> = face_line.split_whitespace().skip(1).collect();
            assert_eq!(
                parts.len(),
                4,
                "each face should have exactly 4 vertex indices"
            );
            for part in parts {
                assert!(
                    part.parse::<usize>().is_ok(),
                    "vertex index should parse as usize"
                );
                assert!(
                    part.parse::<usize>().unwrap() > 0,
                    "vertex index should be >= 1"
                );
            }
        }
    }
}
