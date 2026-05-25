use async_fn_stream::StreamEmitter;
use ndarray::Array2;
use std::fmt::Write;

// This is humungous, we would ordinarily expect this to be <1000
pub const MAX_OBJ_SIZE_USFT: usize = 200_000;

/// Append OBJ geometry for one row of a u16 heightmap into `buf`, advancing `vi`.
///
/// - `skip_pixel(xi, yi, z_in)` → `true` to skip this pixel entirely.
/// - `adj_side_z(adj_idx, adj_raw, z_in)` → given the neighbor's array index (`None` if
///   out-of-bounds), its raw u16 height (0 when OOB), and this pixel's raw u16 height:
///   return `Some(z_ft)` to draw a side face down to that elevation, or `None` to skip.
pub fn append_obj_row(
    xi: usize,
    x_offset: isize,
    y_offset: isize,
    heightmap: &Array2<u16>,
    vi: &mut usize,
    buf: &mut String,
    skip_pixel: impl Fn(usize, usize, u16) -> bool,
    adj_side_z: impl Fn(Option<(usize, usize)>, u16, u16) -> Option<f64>,
) {
    for (yi, &z_in) in heightmap.row(xi).iter().enumerate() {
        if skip_pixel(xi, yi, z_in) {
            continue;
        }
        let z_ft = z_in as f64 / 12.0;
        let x0 = xi as isize + x_offset;
        let y0 = yi as isize + y_offset;
        let x1 = x0 + 1;
        let y1 = y0 + 1;

        let v = *vi + 1;
        *vi += 4;
        let _ = write!(
            buf,
            "v {x0} {y0} {z_ft:.3}\nv {x1} {y0} {z_ft:.3}\n\
             v {x1} {y1} {z_ft:.3}\nv {x0} {y1} {z_ft:.3}\n\
             f {v} {} {} {}\n",
            v + 1,
            v + 2,
            v + 3
        );

        for (dxi, dyi, ax, ay, bx, by) in [
            (0isize, -1isize, x0, y0, x1, y0),
            (0, 1, x1, y1, x0, y1),
            (1, 0, x1, y0, x1, y1),
            (-1, 0, x0, y1, x0, y0),
        ] {
            let adj_idx = xi.checked_add_signed(dxi).zip(yi.checked_add_signed(dyi));
            let adj_raw = adj_idx
                .and_then(|(ax_i, ay_i)| heightmap.get([ax_i, ay_i]).copied())
                .unwrap_or(0);
            let Some(adj_z) = adj_side_z(adj_idx, adj_raw, z_in) else {
                continue;
            };

            let v = *vi + 1;
            *vi += 4;
            let _ = write!(
                buf,
                "v {ax} {ay} {adj_z:.3}\nv {bx} {by} {adj_z:.3}\n\
                 v {bx} {by} {z_ft:.3}\nv {ax} {ay} {z_ft:.3}\n\
                 f {v} {} {} {}\n",
                v + 1,
                v + 2,
                v + 3
            );
        }
    }
}

#[macro_export]
macro_rules! yield_str {
    ($e:expr, $s:literal) => {
        $e.emit($s.to_string()).await
    };
    ($e:expr, $fmt:literal, $($arg:expr),*) => {
        $e.emit(format!($fmt, $($arg),*)).await
    };
}

pub struct RooftopObjWriter<'a> {
    vi: usize,
    emitter: &'a StreamEmitter<String>,
}

impl<'a> RooftopObjWriter<'a> {
    pub fn new(emitter: &'a StreamEmitter<String>) -> Self {
        Self { vi: 0, emitter }
    }

    pub async fn write_vertex(&mut self, x: f64, y: f64, z: f64) -> usize {
        self.vi += 1;
        self.emitter.emit(format!("v {x} {y} {z:.3}\n")).await;
        self.vi
    }

    pub async fn write_horizontal_face(&mut self, x0: f64, x1: f64, y0: f64, y1: f64, z_ft: f64) {
        let vertex_ids = (
            self.write_vertex(x0, y0, z_ft).await,
            self.write_vertex(x1, y0, z_ft).await,
            self.write_vertex(x1, y1, z_ft).await,
            self.write_vertex(x0, y1, z_ft).await,
        );
        self.rect_face(vertex_ids).await;
    }

    pub async fn write_vertical_face(
        &mut self,
        ax: f64,
        bx: f64,
        ay: f64,
        by: f64,
        z_top: f64,
        z_bot: f64,
    ) {
        let vertex_ids = (
            self.write_vertex(ax, ay, z_bot).await,
            self.write_vertex(bx, by, z_bot).await,
            self.write_vertex(bx, by, z_top).await,
            self.write_vertex(ax, ay, z_top).await,
        );
        self.rect_face(vertex_ids).await;
    }

    pub async fn rect_face(&mut self, vertex_ids: (usize, usize, usize, usize)) {
        self.emitter
            .emit(format!(
                "f {} {} {} {}\n",
                vertex_ids.0, vertex_ids.1, vertex_ids.2, vertex_ids.3
            ))
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_fn_stream::fn_stream;
    use futures_util::StreamExt;

    // --- write_vertex ---

    #[tokio::test]
    async fn test_write_vertex_format() {
        let out: Vec<String> = fn_stream(|e| async move {
            RooftopObjWriter::new(&e).write_vertex(1.0, 2.0, 3.5).await;
        })
        .collect()
        .await;
        assert_eq!(out, vec!["v 1 2 3.500\n"]);
    }

    #[tokio::test]
    async fn test_write_vertex_z_precision() {
        // z always uses exactly 3 decimal places regardless of the value
        let out: Vec<String> = fn_stream(|e| async move {
            let mut w = RooftopObjWriter::new(&e);
            w.write_vertex(0.0, 0.0, 1.0).await;
            w.write_vertex(0.0, 0.0, 1.5).await;
            w.write_vertex(0.0, 0.0, 1.123456).await;
        })
        .collect()
        .await;
        assert_eq!(out[0], "v 0 0 1.000\n");
        assert_eq!(out[1], "v 0 0 1.500\n");
        assert_eq!(out[2], "v 0 0 1.123\n");
    }

    #[tokio::test]
    async fn test_write_vertex_returns_sequential_one_based_indices() {
        fn_stream(|e| async move {
            let mut w = RooftopObjWriter::new(&e);
            assert_eq!(w.write_vertex(0.0, 0.0, 0.0).await, 1);
            assert_eq!(w.write_vertex(0.0, 0.0, 0.0).await, 2);
            assert_eq!(w.write_vertex(0.0, 0.0, 0.0).await, 3);
        })
        .collect::<Vec<String>>()
        .await;
    }

    // --- rect_face ---

    #[tokio::test]
    async fn test_rect_face_format() {
        let out: Vec<String> = fn_stream(|e| async move {
            RooftopObjWriter::new(&e).rect_face((3, 7, 12, 1)).await;
        })
        .collect()
        .await;
        assert_eq!(out, vec!["f 3 7 12 1\n"]);
    }

    // --- write_horizontal_face ---

    #[tokio::test]
    async fn test_write_horizontal_face_output() {
        let out: Vec<String> = fn_stream(|e| async move {
            RooftopObjWriter::new(&e)
                .write_horizontal_face(0.0, 1.0, 0.0, 1.0, 5.0)
                .await;
        })
        .collect()
        .await;

        // 4 vertices then 1 face
        assert_eq!(out.len(), 5);
        assert_eq!(out[0], "v 0 0 5.000\n"); // (x0, y0, z)
        assert_eq!(out[1], "v 1 0 5.000\n"); // (x1, y0, z)
        assert_eq!(out[2], "v 1 1 5.000\n"); // (x1, y1, z)
        assert_eq!(out[3], "v 0 1 5.000\n"); // (x0, y1, z)
        assert_eq!(out[4], "f 1 2 3 4\n");
    }

    // --- write_vertical_face ---

    #[tokio::test]
    async fn test_write_vertical_face_output() {
        let out: Vec<String> = fn_stream(|e| async move {
            RooftopObjWriter::new(&e)
                .write_vertical_face(0.0, 1.0, 2.0, 3.0, 10.0, 5.0)
                .await;
        })
        .collect()
        .await;

        assert_eq!(out.len(), 5);
        assert_eq!(out[0], "v 0 2 5.000\n"); // (ax, ay, z_bot)
        assert_eq!(out[1], "v 1 3 5.000\n"); // (bx, by, z_bot)
        assert_eq!(out[2], "v 1 3 10.000\n"); // (bx, by, z_top)
        assert_eq!(out[3], "v 0 2 10.000\n"); // (ax, ay, z_top)
        assert_eq!(out[4], "f 1 2 3 4\n");
    }

    // --- vertex index state ---

    #[tokio::test]
    async fn test_vertex_index_persists_across_face_writes() {
        // After write_horizontal_face uses indices 1-4, the next vertex should be 5.
        fn_stream(|e| async move {
            let mut w = RooftopObjWriter::new(&e);
            w.write_horizontal_face(0.0, 1.0, 0.0, 1.0, 0.0).await;
            let next = w.write_vertex(0.0, 0.0, 0.0).await;
            assert_eq!(next, 5);
        })
        .collect::<Vec<String>>()
        .await;
    }

    #[tokio::test]
    async fn test_face_references_correct_indices_after_prior_vertices() {
        // Vertices written before a face call should be referenced by correct indices.
        let out: Vec<String> = fn_stream(|e| async move {
            let mut w = RooftopObjWriter::new(&e);
            w.write_vertex(0.0, 0.0, 0.0).await; // idx 1 (not part of next face)
            w.write_horizontal_face(1.0, 2.0, 1.0, 2.0, 3.0).await;
        })
        .collect()
        .await;

        // face should reference indices 2,3,4,5
        assert_eq!(out.last().unwrap(), "f 2 3 4 5\n");
    }
}
