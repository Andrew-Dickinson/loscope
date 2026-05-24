use async_fn_stream::{fn_stream, StreamEmitter};
use futures_util::{Stream, StreamExt};
use ndarray::Axis;
use crate::building::heightmap::RooftopHeightMap;
use crate::types::obj_writer::{RooftopObjWriter, MAX_OBJ_SIZE_USFT};
use crate::yield_str;

impl RooftopHeightMap {
    pub async fn to_rooftop_obj_string(&self) -> String {
        self.to_rooftop_obj_stream().fold(String::new(), |mut output, string| async move {
            output.push_str(&string);
            output
        }).await
    }

    pub fn to_rooftop_obj_stream(&self) -> impl Stream<Item = String> {
        // TODO: Should we add a field for this to RooftopHeightMap,
        //  for actual ground approximation?
        let z_ground = 0.0;

        let heightmap = self.heightmap();
        let mask = self.mask();

        let heightmap_ft = heightmap.map(|z_in| f64::from(*z_in) / 12.0);
        assert!(heightmap_ft.nrows() < MAX_OBJ_SIZE_USFT);
        assert!(heightmap_ft.ncols() < MAX_OBJ_SIZE_USFT);

        fn_stream(|e| async move {
            yield_str!(e, "# Building heightmap terrain\n");
            yield_str!(e, "# X = easting (local), Y = northing (local), Z = elevation (ft)\n");
            yield_str!(e, "o heightmap\n\n");

            let mut writer = RooftopObjWriter::new(&e);

            for (xi, col) in heightmap_ft.axis_iter(Axis(0)).into_iter().enumerate() {
                for (yi, z_ft) in col.iter().enumerate() {
                    if !mask.get([xi, yi]).expect("heightmap_ft.shape() != mask.shape()") { continue; }

                    // as f64 is safe per assertions above about
                    // max(xi, yi) = max(nrows, ncols) < MAX_OBJ_SIZE_USFT
                    let (x0, y0) = (xi as f64, yi as f64);
                    let (x1, y1) = (x0 + 1.0, y0 + 1.0);
                    writer.write_horizontal_face(x0, x1, y0, y1, *z_ft).await;

                    // Side faces
                    for (dxi, dyi, ax, ay, bx, by) in [
                        ( 0, -1, x0, y0, x1, y0),
                        ( 0,  1, x1, y1, x0, y1),
                        ( 1,  0, x1, y0, x1, y1),
                        (-1,  0, x0, y1, x0, y0),
                    ] {
                        let (delta_xi, delta_yi): (i8, i8) = (dxi, dyi);
                        let maybe_adj_z = xi.checked_add_signed(delta_xi.into())
                            .zip(yi.checked_add_signed(delta_yi.into()))
                            .filter(|adj_xy| *mask.get([adj_xy.0, adj_xy.1]).unwrap_or(&false))
                            .and_then(|adj_xy| heightmap_ft.get([adj_xy.0, adj_xy.1]));

                        // We trim off the outside edges of the building, and pit-marks
                        // by skipping the side face for any adjecent pixel outside the mask,
                        // or with a height of 0.0 (these are usually data errors sprinked into
                        // the middle of the roof)
                        let adj_z = match maybe_adj_z {
                            Some(&adj_z) if adj_z == 0.0 => { continue;}
                            None => { continue }
                            Some(&adj_z) => { adj_z }
                        };

                        // To avoid duplicate vertical faces, the top face "wins", and we don't draw
                        // the side if the adjacent pixel is below this one
                        if adj_z > *z_ft { continue }

                        writer.write_vertical_face(ax, bx, ay, by, *z_ft, adj_z).await;
                    }
                }
            }
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
