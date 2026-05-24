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