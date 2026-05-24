use async_fn_stream::StreamEmitter;

// This is humungous, we would ordinarily expect this to be <1000
pub const MAX_OBJ_SIZE_USFT: usize = 200_000;

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
            self.write_vertex(x0, y1, z_ft).await
        );
        self.rect_face(vertex_ids).await;
    }

    pub async fn write_vertical_face(&mut self, ax: f64, bx: f64, ay: f64, by: f64, z_top: f64, z_bot: f64) {
        let vertex_ids = (
            self.write_vertex(ax, ay, z_bot).await,
            self.write_vertex(bx, by, z_bot).await,
            self.write_vertex(bx, by, z_top).await,
            self.write_vertex(ax, ay, z_top).await,
        );
        self.rect_face(vertex_ids).await;
    }

    pub async fn rect_face(&mut self, vertex_ids: (usize, usize, usize, usize)) {
        self.emitter.emit(
            format!("f {} {} {} {}\n", vertex_ids.0, vertex_ids.1, vertex_ids.2, vertex_ids.3)
        ).await;
    }
}