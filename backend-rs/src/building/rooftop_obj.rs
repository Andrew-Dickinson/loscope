use async_fn_stream::{fn_stream, StreamEmitter};
use futures_util::{Stream, StreamExt};
use ndarray::Axis;
use crate::building::heightmap::RooftopHeightMap;

// This is humungous, we would ordinarily expect this to be <1000
const MAX_OBJ_SIZE_USFT: usize = 200_000;

macro_rules! yield_str {
    ($e:expr, $s:literal) => {
        $e.emit($s.to_string()).await
    };
    ($e:expr, $fmt:literal, $($arg:expr),*) => {
        $e.emit(format!($fmt, $($arg),*)).await
    };
}

struct RooftopObjWriter<'a> {
    vi: usize,
    emitter: &'a StreamEmitter<String>,
}

impl<'a> RooftopObjWriter<'a> {
    fn new(emitter: &'a StreamEmitter<String>) -> Self {
        Self { vi: 0, emitter }
    }

    async fn write_vertex(&mut self, x: f64, y: f64, z: f64) -> usize {
        self.vi += 1;
        self.emitter.emit(format!("v {x} {y} {z:.3}\n")).await;
        self.vi
    }

    async fn write_horizontal_face(&mut self, x0: f64, x1: f64, y0: f64, y1: f64, z_ft: f64) {
        let vertex_ids = (
            self.write_vertex(x0, y0, z_ft).await,
            self.write_vertex(x1, y0, z_ft).await,
            self.write_vertex(x1, y1, z_ft).await,
            self.write_vertex(x0, y1, z_ft).await
        );
        self.rect_face(vertex_ids).await;
    }

    async fn write_vertical_face(&mut self, ax: f64, bx: f64, ay: f64, by: f64, z_top: f64, z_bot: f64) {
        let vertex_ids = (
            self.write_vertex(ax, ay, z_bot).await,
            self.write_vertex(bx, by, z_bot).await,
            self.write_vertex(bx, by, z_top).await,
            self.write_vertex(ax, ay, z_top).await,
        );
        self.rect_face(vertex_ids).await;
    }

    async fn rect_face(&mut self, vertex_ids: (usize, usize, usize, usize)) {
        self.emitter.emit(
            format!("f {} {} {} {}\n", vertex_ids.0, vertex_ids.1, vertex_ids.2, vertex_ids.3)
        ).await;
    }
}

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