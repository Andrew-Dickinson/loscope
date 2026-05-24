use async_fn_stream::fn_stream;
use futures_util::Stream;
use ndarray::Axis;
use uuid::Uuid;
use crate::analysis::fresnel_zone::{FresnelZone};
use crate::types::obj_writer::RooftopObjWriter;
use crate::types::tiles::TileId;
use crate::yield_str;

pub fn stream_fresnel_tile_slice_as_obj(analysis_id: Uuid, fresnel_zone: &FresnelZone, tile_id: TileId) -> impl Stream<Item = String> {
    let fresnel_raster = fresnel_zone.rasterize_in_tile(tile_id).mapv(|opt| opt.copied());

    fn_stream(|e| async move {
        yield_str!(e, "# Fresnel zone slice \n");
        e.emit(format!("# Analysis id: {}\n", analysis_id)).await;
        e.emit( format!("# Tile Id: {}\n", &tile_id)).await;
        yield_str!(e, "# X = easting (within tile), Y = northing (within tile), Z = elevation (ft)\n");
        yield_str!(e, "o fresnel_zone\n\n");

        let mut writer = RooftopObjWriter::new(&e);

        for (xi, col) in fresnel_raster.axis_iter(Axis(0)).into_iter().enumerate() {
            for (yi, maybe_zone_point) in col.iter().enumerate() {
                let &Some(zone_point) = maybe_zone_point else { continue; };

                let local_top = f64::from(zone_point.top()) / 12.0;
                let local_bot = f64::from(zone_point.bottom()) / 12.0;

                // as f64 is safe per assertions above about
                // max(xi, yi) = max(nrows, ncols) < MAX_OBJ_SIZE_USFT
                let (x0, y0) = (xi as f64, yi as f64);
                let (x1, y1) = (x0 + 1.0, y0 + 1.0);
                writer.write_horizontal_face(x0, x1, y0, y1, local_top).await;
                writer.write_horizontal_face(x0, x1, y0, y1, local_bot).await;

                // Side faces
                for (dxi, dyi, ax, ay, bx, by) in [
                    ( 0, -1, x0, y0, x1, y0),
                    ( 0,  1, x1, y1, x0, y1),
                    ( 1,  0, x1, y0, x1, y1),
                    (-1,  0, x0, y1, x0, y0),
                ] {
                    let (delta_xi, delta_yi): (i8, i8) = (dxi, dyi);
                    let maybe_adj_point = xi.checked_add_signed(delta_xi.into())
                        .zip(yi.checked_add_signed(delta_yi.into()))
                        .and_then(|adj_xy| fresnel_raster.get([adj_xy.0, adj_xy.1]).cloned())
                        .flatten();

                    match maybe_adj_point {
                        Some(adj_point) => {
                            let adj_top = f64::from(adj_point.top()) / 12.0;
                            let adj_bottom = f64::from(adj_point.bottom()) / 12.0;

                            // To avoid duplicate vertical faces, the top face "wins", and we don't draw
                            // the side if the adjacent pixel is below this one
                            if adj_top >= local_top {
                                writer.write_vertical_face(ax, bx, ay, by, adj_top, local_top).await;
                            }
                            if adj_bottom >= local_bot {
                                writer.write_vertical_face(ax, bx, ay, by, adj_bottom, local_bot).await;
                            }
                        }
                        None => writer.write_vertical_face(ax, bx, ay, by, local_top, local_bot).await,
                    };
                }
            }
        }
    })
}