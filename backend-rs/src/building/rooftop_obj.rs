use async_fn_stream::fn_stream;
use futures_util::{Stream, StreamExt};
use crate::building::heightmap::RooftopHeightMap;
use crate::types::obj_writer::{append_obj_row, MAX_OBJ_SIZE_USFT};

impl RooftopHeightMap {
    pub async fn to_rooftop_obj_string(&self) -> String {
        self.to_rooftop_obj_stream().fold(String::new(), |mut output, string| async move {
            output.push_str(&string);
            output
        }).await
    }

    pub fn to_rooftop_obj_stream(&self) -> impl Stream<Item = String> {
        let heightmap = self.heightmap().clone();
        let mask = self.mask().clone();
        assert!(heightmap.nrows() < MAX_OBJ_SIZE_USFT);
        assert!(heightmap.ncols() < MAX_OBJ_SIZE_USFT);

        fn_stream(|e| async move {
            e.emit(
                "# Building heightmap terrain\n\
                 # X = easting (local), Y = northing (local), Z = elevation (ft)\n\
                 o heightmap\n\n"
                .to_string()
            ).await;

            let mut vi: usize = 0;
            let mut buf = String::with_capacity(16 * 1024);

            for xi in 0..heightmap.nrows() {
                append_obj_row(
                    xi, 0, 0, &heightmap, &mut vi, &mut buf,
                    |xi, yi, _z_in| !mask[[xi, yi]],
                    // Side face only when the neighbor is in the mask, non-zero, and not taller.
                    // Outside-mask neighbors (adj_idx=None or mask=false) are trimmed edges — skip.
                    // Zero-height in-mask neighbors are data errors — skip.
                    |adj_idx, adj_raw, z_in| {
                        let (adj_xi, adj_yi) = adj_idx?;
                        if !mask.get([adj_xi, adj_yi]).copied().unwrap_or(false) { return None; }
                        if adj_raw == 0 { return None; }
                        if adj_raw > z_in { return None; }
                        Some(adj_raw as f64 / 12.0)
                    },
                );
                if buf.len() >= 16 * 1024 {
                    e.emit(std::mem::take(&mut buf)).await;
                }
            }

            if !buf.is_empty() { e.emit(buf).await; }
        })
    }
}


#[cfg(test)]
mod tests {
    use ndarray::Array2;
    use futures_util::StreamExt;
    use crate::building::bin_id::BINId;
    use crate::building::heightmap::RooftopHeightMap;
    use crate::types::coords::NYSCoords2;

    fn bin_id() -> BINId { BINId::parse("1234567").unwrap() }
    fn sw() -> NYSCoords2 { NYSCoords2::new(0.0, 0.0) }
    fn dummy_poly() -> geo::Polygon {
        geo::Polygon::new(
            geo::LineString::from(vec![(0.0f64, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)]),
            vec![],
        )
    }

    fn make_hmap(heights_in: Vec<u16>, mask: Vec<bool>, shape: (usize, usize)) -> RooftopHeightMap {
        RooftopHeightMap::new(
            bin_id(),
            sw(),
            Array2::from_shape_vec(shape, heights_in).unwrap(),
            Array2::from_shape_vec(shape, mask).unwrap(),
            dummy_poly(),
        )
    }

    async fn collect(hmap: &RooftopHeightMap) -> String {
        hmap.to_rooftop_obj_stream()
            .fold(String::new(), |mut s, chunk| async move { s.push_str(&chunk); s })
            .await
    }

    fn count_lines_starting_with(obj: &str, prefix: &str) -> usize {
        obj.lines().filter(|l| l.starts_with(prefix)).count()
    }

    #[tokio::test]
    async fn header_lines_are_present() {
        let hmap = make_hmap(vec![], vec![], (0, 0));
        // 0-size array would panic the assert in the function, use 1x1 all-false instead
        let hmap = make_hmap(vec![0u16], vec![false], (1, 1));
        let obj = collect(&hmap).await;
        assert!(obj.contains("# Building heightmap terrain"));
        assert!(obj.contains("# X = easting (local), Y = northing (local), Z = elevation (ft)"));
        assert!(obj.contains("o heightmap"));
    }

    #[tokio::test]
    async fn all_mask_false_produces_no_geometry() {
        let hmap = make_hmap(vec![120u16, 240], vec![false, false], (1, 2));
        let obj = collect(&hmap).await;
        assert_eq!(count_lines_starting_with(&obj, "v "), 0, "no vertices expected");
        assert_eq!(count_lines_starting_with(&obj, "f "), 0, "no faces expected");
    }

    #[tokio::test]
    async fn single_pixel_produces_one_horizontal_face() {
        // 1 ft = 12 inches; single pixel, no neighbours → only top face, no side faces
        let hmap = make_hmap(vec![12u16], vec![true], (1, 1));
        let obj = collect(&hmap).await;
        assert_eq!(count_lines_starting_with(&obj, "v "), 4, "4 vertices for 1 face");
        assert_eq!(count_lines_starting_with(&obj, "f "), 1, "1 face");
    }

    #[tokio::test]
    async fn single_pixel_vertex_coords_are_correct() {
        let hmap = make_hmap(vec![24u16], vec![true], (1, 1)); // 24 in = 2.0 ft
        let obj = collect(&hmap).await;
        let vertices: Vec<&str> = obj.lines().filter(|l| l.starts_with("v ")).collect();
        // write_horizontal_face emits (x0,y0,z),(x1,y0,z),(x1,y1,z),(x0,y1,z) = corners of [0,1]×[0,1]
        assert!(vertices.iter().any(|&v| v == "v 0 0 2.000"), "missing v 0 0 2.000 in {:?}", vertices);
        assert!(vertices.iter().any(|&v| v == "v 1 0 2.000"), "missing v 1 0 2.000");
        assert!(vertices.iter().any(|&v| v == "v 1 1 2.000"), "missing v 1 1 2.000");
        assert!(vertices.iter().any(|&v| v == "v 0 1 2.000"), "missing v 0 1 2.000");
    }

    #[tokio::test]
    async fn taller_pixel_draws_side_face_toward_shorter_neighbor() {
        // shape (2,1): pixels at [0,0]=tall, [1,0]=short, adjacent in x direction
        // Tall pixel: adj_z < z_ft → draw side face
        // Short pixel: adj_z > z_ft → skip
        let hmap = make_hmap(
            vec![240u16, 120u16],   // 20 ft, 10 ft
            vec![true, true],
            (2, 1),
        );
        let obj = collect(&hmap).await;
        let v_count = count_lines_starting_with(&obj, "v ");
        let f_count = count_lines_starting_with(&obj, "f ");
        // Tall: 1 top face (4v+1f) + 1 side face toward shorter (4v+1f) = 8v + 2f
        // Short: 1 top face (4v+1f) = 4v + 1f
        // Total: 12v + 3f
        assert_eq!(v_count, 12, "vertices: {:?}", v_count);
        assert_eq!(f_count, 3, "faces");
    }

    #[tokio::test]
    async fn adjacent_pixel_with_zero_height_suppresses_side_face() {
        // [0,0]=tall, [1,0]=z_in_mask_but_z=0 → adj_z=0.0 branch → continue, no side face
        let hmap = make_hmap(
            vec![240u16, 0u16],
            vec![true, true],
            (2, 1),
        );
        let obj = collect(&hmap).await;
        // Tall pixel: adj_z=0 → skip side face. Zero pixel: draws a face (mask=true, z=0 is valid for self).
        // Actually: the zero-height pixel also gets processed (mask=true), but its adj (the tall one)
        // has adj_z=2.0 > z_ft=0.0 → skip its side too.
        let f_count = count_lines_starting_with(&obj, "f ");
        // Both pixels get only top faces (1 each), no side faces between them → 2 faces
        assert_eq!(f_count, 2, "only top faces, no side faces");
    }

    #[tokio::test]
    async fn equal_height_adjacent_pixels_both_draw_shared_side() {
        // Both pixels same height: adj_z == z_ft → adj_z > z_ft is false → both draw side face
        let hmap = make_hmap(
            vec![120u16, 120u16],
            vec![true, true],
            (2, 1),
        );
        let obj = collect(&hmap).await;
        // Pixel 0: top face + side toward pixel 1 = 2 faces
        // Pixel 1: top face + side toward pixel 0 = 2 faces
        // Total: 4 faces
        let f_count = count_lines_starting_with(&obj, "f ");
        assert_eq!(f_count, 4);
    }

    #[tokio::test]
    async fn to_rooftop_obj_string_equals_collected_stream() {
        let hmap = make_hmap(vec![60u16], vec![true], (1, 1));
        let from_string = hmap.to_rooftop_obj_string().await;
        let from_stream = collect(&hmap).await;
        assert_eq!(from_string, from_stream);
    }
}
