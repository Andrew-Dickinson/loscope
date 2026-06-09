use std::path::Path;

use anyhow::{Context, Result};
#[allow(deprecated)]
use geo::{BoundingRect, Contains, EuclideanDistance, MultiPolygon, Point, Polygon, Rect};
use loscope::types::coords::GPSCoords2;
use loscope::util::coord_conversion::CoordinateConverter;
use wkt::TryFromWkt;

use crate::preprocess::rasterize::{HeightGrid, VegGrid, GRID_SIDE};

const VEG_Z_TOLERANCE_USFT: f64 = 5.0;
const BUFFER_USFT: f64 = 5.0;

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PixelClass {
    None = 0,
    Vegetation = 1,
    Building = 2,
    Water = 3,
}

pub type ClassGrid = Vec<u8>;

// ── OSM loading ──────────────────────────────────────────────────────────────

/// Load land polygons from an OSM land-polygons shapefile (WGS84).
/// Returns (bounding_rect, polygon) pairs in EPSG 6539 (NYS LI, US survey feet).
pub fn load_osm_land_polys(path: &Path) -> Result<Vec<(Rect<f64>, Polygon<f64>)>> {
    let converter = CoordinateConverter::new();
    let mut reader = shapefile::Reader::from_path(path)
        .with_context(|| format!("Cannot open shapefile: {}", path.display()))?;

    let mut polys: Vec<(Rect<f64>, Polygon<f64>)> = Vec::new();
    for result in reader.iter_shapes_and_records() {
        let (shape, _) = result
            .with_context(|| format!("Error reading shapefile: {}", path.display()))?;
        let sf_poly = match shape {
            shapefile::Shape::Polygon(p) => p,
            _ => continue,
        };
        for poly in shapefile_polygon_to_geo(sf_poly, &converter) {
            if let Some(bbox) = poly.bounding_rect() {
                polys.push((bbox, poly));
            }
        }
    }
    Ok(polys)
}

/// Load hydro-structure polygons from an OSM GeoJSON file (WGS84).
/// Returns (bounding_rect, polygon) pairs in EPSG 6539 (NYS LI, US survey feet).
pub fn load_osm_hydro_structures(path: &Path) -> Result<Vec<(Rect<f64>, Polygon<f64>)>> {
    let converter = CoordinateConverter::new();
    let json_str = std::fs::read_to_string(path)
        .with_context(|| format!("Cannot read GeoJSON: {}", path.display()))?;
    let geojson: geojson::GeoJson = json_str
        .parse()
        .with_context(|| format!("Cannot parse GeoJSON: {}", path.display()))?;

    let mut polys: Vec<(Rect<f64>, Polygon<f64>)> = Vec::new();
    let features = match geojson {
        geojson::GeoJson::FeatureCollection(fc) => fc.features,
        geojson::GeoJson::Feature(f) => vec![f],
        geojson::GeoJson::Geometry(g) => {
            extract_geojson_polygons(g, &converter, &mut polys)?;
            return Ok(polys);
        }
    };

    for feature in features {
        if let Some(geom) = feature.geometry {
            extract_geojson_polygons(geom, &converter, &mut polys)?;
        }
    }
    Ok(polys)
}

// ── Database loading ──────────────────────────────────────────────────────────

/// Load all building footprint polygons from the `building_footprints` table.
/// Returns (bounding_rect, polygon) pairs for spatial pre-filtering.
pub fn load_building_footprints(db_path: &Path) -> Result<Vec<(Rect<f64>, Polygon<f64>)>> {
    use rusqlite::{Connection, OpenFlags};

    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("Cannot open DB: {}", db_path.display()))?;
    conn.execute_batch("PRAGMA cache_size=-65536; PRAGMA temp_store=MEMORY;")?;

    let mut stmt = conn.prepare(
        "SELECT the_geom FROM building_footprints WHERE the_geom IS NOT NULL AND the_geom != ''",
    )?;
    let mut rows = stmt.query([])?;

    let mut polys = Vec::new();
    while let Some(row) = rows.next()? {
        let wkt_str: String = row.get(0)?;
        let poly = match parse_polygon_wkt(&wkt_str) {
            Some(p) => p,
            None => continue,
        };
        if let Some(bbox) = poly.bounding_rect() {
            polys.push((bbox, poly));
        }
    }
    Ok(polys)
}

// ── Planimetrics CSV loading ──────────────────────────────────────────────────

/// Load polygons from a planimetrics CSV that has a `the_geom` WKT column (EPSG 6539).
pub fn load_planimetrics_csv(path: &Path) -> Result<Vec<(Rect<f64>, Polygon<f64>)>> {
    let mut rdr = csv::Reader::from_path(path)
        .with_context(|| format!("Cannot open CSV: {}", path.display()))?;

    let headers = rdr.headers()?.clone();
    let geom_col = headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case("the_geom"))
        .ok_or_else(|| anyhow::anyhow!("No 'the_geom' column in {}", path.display()))?;

    let mut polys = Vec::new();
    for result in rdr.records() {
        let record = result?;
        let wkt_str = match record.get(geom_col) {
            Some(s) if !s.is_empty() => s,
            _ => continue,
        };
        let poly = match parse_polygon_wkt(wkt_str) {
            Some(p) => p,
            None => continue,
        };
        if let Some(bbox) = poly.bounding_rect() {
            polys.push((bbox, poly));
        }
    }
    Ok(polys)
}

// ── Spatial filtering ────────────────────────────────────────────────────────

/// Filter a pre-loaded list of (bbox, poly) pairs to those whose bbox overlaps the given tile.
pub fn filter_polys_for_tile<'a>(
    all_polys: &'a [(Rect<f64>, Polygon<f64>)],
    tile_bbox: Rect<f64>,
) -> Vec<&'a Polygon<f64>> {
    all_polys
        .iter()
        .filter(|(bbox, _)| bboxes_overlap(bbox, &tile_bbox))
        .map(|(_, p)| p)
        .collect()
}

pub fn tile_bbox(las_id: loscope::types::tiles::LASTileId) -> Rect<f64> {
    let origin = las_id.get_sw_corner();
    let min_e = *origin.easting();
    let min_n = *origin.northing();
    Rect::new(
        geo::coord! { x: min_e, y: min_n },
        geo::coord! { x: min_e + GRID_SIDE as f64, y: min_n + GRID_SIDE as f64 },
    )
}

// ── Classification ────────────────────────────────────────────────────────────

/// Classify every pixel in a LAS tile's 2500×2500 grid.
///
/// Precedence (highest first):
/// 1. Building – pixel center is inside any building polygon
/// 2. Vegetation – a veg LiDAR point within 5 usft of the cell max exists AND the pixel
///    is not within 5 usft of any building or misc-structure polygon boundary
/// 3. Water – pixel center is outside all land polygons AND not inside any hydro-structure
///    polygon (hydro structures such as piers/breakwaters are excluded from water)
/// 4. None
pub fn build_class_grid(
    height_grid: &HeightGrid,
    veg_grid: &VegGrid,
    building_polys: &[&Polygon<f64>],
    misc_structure_polys: &[&Polygon<f64>],
    hydro_polys: &[&Polygon<f64>],
    land_polys: &[&Polygon<f64>],
    las_id: loscope::types::tiles::LASTileId,
) -> ClassGrid {
    let origin = las_id.get_sw_corner();
    let origin_e = *origin.easting();
    let origin_n = *origin.northing();

    let (building_mask, building_buffer_mask) =
        build_poly_masks(building_polys, origin_e, origin_n, GRID_SIDE, BUFFER_USFT);
    let misc_buffer_mask =
        build_buffer_only_mask(misc_structure_polys, origin_e, origin_n, GRID_SIDE, BUFFER_USFT);
    let land_mask = build_containment_mask(land_polys, origin_e, origin_n, GRID_SIDE);
    let hydro_mask = build_containment_mask(hydro_polys, origin_e, origin_n, GRID_SIDE);

    let mut class_grid = vec![PixelClass::None as u8; GRID_SIDE * GRID_SIDE];

    for x in 0..GRID_SIDE {
        for y in 0..GRID_SIDE {
            let idx = x * GRID_SIDE + y;

            // Rule 1: building
            if building_mask[idx] {
                class_grid[idx] = PixelClass::Building as u8;
                continue;
            }

            // Rule 2: vegetation
            let max_veg = veg_grid[idx];
            if max_veg > 0.0 && max_veg >= height_grid[idx] - VEG_Z_TOLERANCE_USFT {
                let in_buffered_obstruction =
                    building_buffer_mask[idx] || misc_buffer_mask[idx];
                if !in_buffered_obstruction {
                    class_grid[idx] = PixelClass::Vegetation as u8;
                    continue;
                }
            }

            // Rule 3: water – outside all land polygons, and not inside a hydro structure
            if !land_mask[idx] && !hydro_mask[idx] {
                class_grid[idx] = PixelClass::Water as u8;
            }

            // Rule 4: None (already the default)
        }
    }

    class_grid
}

// ── Rasterization helpers ─────────────────────────────────────────────────────

/// Rasterize `polys` into a containment mask and a `buffer`-ft dilated mask.
fn build_poly_masks(
    polys: &[&Polygon<f64>],
    origin_e: f64,
    origin_n: f64,
    side: usize,
    buffer: f64,
) -> (Vec<bool>, Vec<bool>) {
    let mut mask = vec![false; side * side];
    let mut buf_mask = vec![false; side * side];
    for poly in polys {
        stamp_poly(poly, &mut mask, &mut buf_mask, origin_e, origin_n, side, buffer);
    }
    (mask, buf_mask)
}

/// Rasterize `polys` into only a containment mask (no buffer zone).
fn build_containment_mask(
    polys: &[&Polygon<f64>],
    origin_e: f64,
    origin_n: f64,
    side: usize,
) -> Vec<bool> {
    let mut mask = vec![false; side * side];
    for poly in polys {
        let bbox = match poly.bounding_rect() {
            Some(b) => b,
            None => continue,
        };
        let x0 = tile_clamp(bbox.min().x - origin_e, side);
        let y0 = tile_clamp(bbox.min().y - origin_n, side);
        let x1 = tile_clamp(bbox.max().x - origin_e + 1.0, side);
        let y1 = tile_clamp(bbox.max().y - origin_n + 1.0, side);
        for xi in x0..x1 {
            for yi in y0..y1 {
                let idx = xi * side + yi;
                if !mask[idx] {
                    let center =
                        Point::new(origin_e + xi as f64 + 0.5, origin_n + yi as f64 + 0.5);
                    if poly.contains(&center) {
                        mask[idx] = true;
                    }
                }
            }
        }
    }
    mask
}

/// Rasterize `polys` into only a `buffer`-ft dilated mask (no containment mask needed).
fn build_buffer_only_mask(
    polys: &[&Polygon<f64>],
    origin_e: f64,
    origin_n: f64,
    side: usize,
    buffer: f64,
) -> Vec<bool> {
    let mut buf_mask = vec![false; side * side];
    let mut dummy = vec![false; side * side];
    for poly in polys {
        stamp_poly(poly, &mut dummy, &mut buf_mask, origin_e, origin_n, side, buffer);
    }
    buf_mask
}

fn stamp_poly(
    poly: &Polygon<f64>,
    mask: &mut Vec<bool>,
    buf_mask: &mut Vec<bool>,
    origin_e: f64,
    origin_n: f64,
    side: usize,
    buffer: f64,
) {
    let bbox = match poly.bounding_rect() {
        Some(b) => b,
        None => return,
    };

    let x0 = tile_clamp(bbox.min().x - origin_e, side);
    let y0 = tile_clamp(bbox.min().y - origin_n, side);
    let x1 = tile_clamp(bbox.max().x - origin_e + 1.0, side);
    let y1 = tile_clamp(bbox.max().y - origin_n + 1.0, side);

    let bx0 = tile_clamp(bbox.min().x - origin_e - buffer, side);
    let by0 = tile_clamp(bbox.min().y - origin_n - buffer, side);
    let bx1 = tile_clamp(bbox.max().x - origin_e + buffer + 1.0, side);
    let by1 = tile_clamp(bbox.max().y - origin_n + buffer + 1.0, side);

    // Containment pass (normal bbox)
    for xi in x0..x1 {
        for yi in y0..y1 {
            let idx = xi * side + yi;
            if !mask[idx] {
                let center =
                    Point::new(origin_e + xi as f64 + 0.5, origin_n + yi as f64 + 0.5);
                if poly.contains(&center) {
                    mask[idx] = true;
                    buf_mask[idx] = true; // inside ⊆ buffered
                }
            }
        }
    }

    // Buffer pass (expanded bbox)
    for xi in bx0..bx1 {
        for yi in by0..by1 {
            let idx = xi * side + yi;
            if !buf_mask[idx] {
                let center =
                    Point::new(origin_e + xi as f64 + 0.5, origin_n + yi as f64 + 0.5);
                #[allow(deprecated)]
                if center.euclidean_distance(poly) <= buffer {
                    buf_mask[idx] = true;
                }
            }
        }
    }
}

fn tile_clamp(v: f64, side: usize) -> usize {
    v.floor().max(0.0).min(side as f64) as usize
}

fn bboxes_overlap(a: &Rect<f64>, b: &Rect<f64>) -> bool {
    a.min().x < b.max().x
        && a.max().x > b.min().x
        && a.min().y < b.max().y
        && a.max().y > b.min().y
}

// ── WKT / shapefile / GeoJSON parsing ────────────────────────────────────────

fn parse_polygon_wkt(wkt_str: &str) -> Option<Polygon<f64>> {
    if let Ok(p) = Polygon::<f64>::try_from_wkt_str(wkt_str) {
        return Some(p);
    }
    if let Ok(mp) = MultiPolygon::<f64>::try_from_wkt_str(wkt_str) {
        return mp.0.into_iter().next();
    }
    None
}

fn shapefile_polygon_to_geo(
    sf_poly: shapefile::Polygon,
    converter: &CoordinateConverter,
) -> Vec<Polygon<f64>> {
    let rings = sf_poly.rings();
    let mut result: Vec<Polygon<f64>> = Vec::new();
    let mut current_exterior: Option<geo::LineString<f64>> = None;
    let mut current_holes: Vec<geo::LineString<f64>> = Vec::new();

    for ring in rings {
        match ring {
            shapefile::PolygonRing::Outer(pts) => {
                if let Some(ext) = current_exterior.take() {
                    result.push(Polygon::new(ext, std::mem::take(&mut current_holes)));
                }
                current_exterior = Some(shapefile_pts_to_nys_ls(pts, converter));
            }
            shapefile::PolygonRing::Inner(pts) => {
                current_holes.push(shapefile_pts_to_nys_ls(pts, converter));
            }
        }
    }
    if let Some(ext) = current_exterior {
        result.push(Polygon::new(ext, current_holes));
    }
    result
}

fn shapefile_pts_to_nys_ls(
    pts: &[shapefile::Point],
    converter: &CoordinateConverter,
) -> geo::LineString<f64> {
    geo::LineString::from(
        pts.iter()
            .map(|p| {
                // shapefile: x = lon, y = lat
                let nys = converter.to_nys_plane2(&GPSCoords2::new(p.y, p.x));
                geo::coord! { x: *nys.easting(), y: *nys.northing() }
            })
            .collect::<Vec<_>>(),
    )
}

fn extract_geojson_polygons(
    geom: geojson::Geometry,
    converter: &CoordinateConverter,
    out: &mut Vec<(Rect<f64>, Polygon<f64>)>,
) -> Result<()> {
    match geom.value {
        geojson::GeometryValue::Polygon { coordinates: rings } => {
            if let Some(poly) = geojson_poly_rings_to_nys(rings, converter) {
                if let Some(bbox) = poly.bounding_rect() {
                    out.push((bbox, poly));
                }
            }
        }
        geojson::GeometryValue::MultiPolygon { coordinates: multi_rings } => {
            for rings in multi_rings {
                if let Some(poly) = geojson_poly_rings_to_nys(rings, converter) {
                    if let Some(bbox) = poly.bounding_rect() {
                        out.push((bbox, poly));
                    }
                }
            }
        }
        geojson::GeometryValue::GeometryCollection { geometries: geoms } => {
            for g in geoms {
                extract_geojson_polygons(g, converter, out)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn geojson_poly_rings_to_nys(
    rings: geojson::PolygonType,
    converter: &CoordinateConverter,
) -> Option<Polygon<f64>> {
    let mut iter = rings.into_iter();
    let exterior = geojson_ring_to_nys_ls(iter.next()?, converter);
    let holes: Vec<geo::LineString<f64>> =
        iter.map(|r| geojson_ring_to_nys_ls(r, converter)).collect();
    Some(Polygon::new(exterior, holes))
}

fn geojson_ring_to_nys_ls(
    ring: Vec<geojson::Position>,
    converter: &CoordinateConverter,
) -> geo::LineString<f64> {
    geo::LineString::from(
        ring.into_iter()
            .map(|pos| {
                // GeoJSON: [lon, lat, ...]
                let nys = converter.to_nys_plane2(&GPSCoords2::new(pos[1], pos[0]));
                geo::coord! { x: *nys.easting(), y: *nys.northing() }
            })
            .collect::<Vec<_>>(),
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use geo::polygon;
    use loscope::types::tiles::LASTileId;

    fn las_id() -> LASTileId {
        LASTileId::parse("500300").unwrap()
    }

    fn origin() -> (f64, f64) {
        let sw = las_id().get_sw_corner();
        (*sw.easting(), *sw.northing())
    }

    fn empty_grids() -> (HeightGrid, VegGrid) {
        (vec![0.0; GRID_SIDE * GRID_SIDE], vec![0.0; GRID_SIDE * GRID_SIDE])
    }

    /// Land polygon covering the whole tile + margin – no pixel is outside.
    fn whole_tile_land_poly(id: LASTileId) -> Vec<(Rect<f64>, Polygon<f64>)> {
        let sw = id.get_sw_corner();
        let (oe, on) = (*sw.easting(), *sw.northing());
        let s = GRID_SIDE as f64;
        let poly = polygon![
            (x: oe - 1.0,     y: on - 1.0),
            (x: oe + s + 1.0, y: on - 1.0),
            (x: oe + s + 1.0, y: on + s + 1.0),
            (x: oe - 1.0,     y: on + s + 1.0),
            (x: oe - 1.0,     y: on - 1.0),
        ];
        vec![(poly.bounding_rect().unwrap(), poly)]
    }

    /// Empty land polygon list – every pixel is outside land (potential water).
    fn no_land() -> Vec<(Rect<f64>, Polygon<f64>)> {
        vec![]
    }

    fn filter<'a>(all: &'a [(Rect<f64>, Polygon<f64>)]) -> Vec<&'a Polygon<f64>> {
        all.iter().map(|(_, p)| p).collect()
    }

    #[test]
    fn pixel_inside_building_is_building() {
        let (oe, on) = origin();
        let (height, veg) = empty_grids();

        let building = polygon![
            (x: oe + 5.0,  y: on + 5.0),
            (x: oe + 15.0, y: on + 5.0),
            (x: oe + 15.0, y: on + 15.0),
            (x: oe + 5.0,  y: on + 15.0),
            (x: oe + 5.0,  y: on + 5.0),
        ];
        let land = whole_tile_land_poly(las_id());
        let grid = build_class_grid(
            &height, &veg,
            &[&building], &[], &[], &filter(&land),
            las_id(),
        );

        assert_eq!(grid[10 * GRID_SIDE + 10], PixelClass::Building as u8);
    }

    #[test]
    fn pixel_outside_building_is_not_building() {
        let (oe, on) = origin();
        let (height, veg) = empty_grids();

        let building = polygon![
            (x: oe + 5.0,  y: on + 5.0),
            (x: oe + 15.0, y: on + 5.0),
            (x: oe + 15.0, y: on + 15.0),
            (x: oe + 5.0,  y: on + 15.0),
            (x: oe + 5.0,  y: on + 5.0),
        ];
        let land = whole_tile_land_poly(las_id());
        let grid = build_class_grid(
            &height, &veg,
            &[&building], &[], &[], &filter(&land),
            las_id(),
        );

        assert_ne!(grid[0], PixelClass::Building as u8);
    }

    #[test]
    fn pixel_with_veg_near_top_is_vegetation() {
        let mut height = vec![0.0; GRID_SIDE * GRID_SIDE];
        let mut veg = vec![0.0; GRID_SIDE * GRID_SIDE];

        let idx = 100 * GRID_SIDE + 100;
        height[idx] = 50.0;
        veg[idx] = 47.0; // 3 usft below max → within 5 usft tolerance

        let land = whole_tile_land_poly(las_id());
        let grid = build_class_grid(
            &height, &veg,
            &[], &[], &[], &filter(&land),
            las_id(),
        );

        assert_eq!(grid[idx], PixelClass::Vegetation as u8);
    }

    #[test]
    fn veg_point_too_low_is_not_vegetation() {
        let mut height = vec![0.0; GRID_SIDE * GRID_SIDE];
        let mut veg = vec![0.0; GRID_SIDE * GRID_SIDE];

        let idx = 100 * GRID_SIDE + 100;
        height[idx] = 50.0;
        veg[idx] = 44.0; // 6 usft below max → outside tolerance

        let land = whole_tile_land_poly(las_id());
        let grid = build_class_grid(
            &height, &veg,
            &[], &[], &[], &filter(&land),
            las_id(),
        );

        assert_ne!(grid[idx], PixelClass::Vegetation as u8);
    }

    #[test]
    fn veg_pixel_within_buffer_of_building_is_not_vegetation() {
        let (oe, on) = origin();
        let mut height = vec![0.0; GRID_SIDE * GRID_SIDE];
        let mut veg = vec![0.0; GRID_SIDE * GRID_SIDE];

        let idx = 3 * GRID_SIDE + 3;
        height[idx] = 50.0;
        veg[idx] = 48.0;

        // Building at (7..17, 7..17) – 3 usft from pixel centre (3.5, 3.5) → inside buffer
        let building = polygon![
            (x: oe + 7.0,  y: on + 7.0),
            (x: oe + 17.0, y: on + 7.0),
            (x: oe + 17.0, y: on + 17.0),
            (x: oe + 7.0,  y: on + 17.0),
            (x: oe + 7.0,  y: on + 7.0),
        ];
        let land = whole_tile_land_poly(las_id());
        let grid = build_class_grid(
            &height, &veg,
            &[&building], &[], &[], &filter(&land),
            las_id(),
        );

        assert_ne!(grid[idx], PixelClass::Vegetation as u8);
    }

    #[test]
    fn pixel_outside_land_is_water() {
        let (height, veg) = empty_grids();

        // No land polygons → entire tile is outside land → all water
        let grid = build_class_grid(
            &height, &veg,
            &[], &[], &[], &[],
            las_id(),
        );

        assert!(grid.iter().all(|&c| c == PixelClass::Water as u8));
    }

    #[test]
    fn hydro_structure_suppresses_water_classification() {
        let (oe, on) = origin();
        let (height, veg) = empty_grids();

        // A hydro structure (pier/breakwater) at (5..10, 5..10) – should NOT be water
        let hydro = polygon![
            (x: oe + 5.0,  y: on + 5.0),
            (x: oe + 10.0, y: on + 5.0),
            (x: oe + 10.0, y: on + 10.0),
            (x: oe + 5.0,  y: on + 10.0),
            (x: oe + 5.0,  y: on + 5.0),
        ];
        // No land polys → everything is potential water
        let grid = build_class_grid(
            &height, &veg,
            &[], &[], &[&hydro], &[],
            las_id(),
        );

        // Pixel inside the hydro structure should NOT be Water
        assert_ne!(grid[7 * GRID_SIDE + 7], PixelClass::Water as u8);
        // Pixel outside the hydro structure should still be Water
        assert_eq!(grid[100 * GRID_SIDE + 100], PixelClass::Water as u8);
    }

    #[test]
    fn building_takes_precedence_over_water() {
        let (oe, on) = origin();
        let (height, veg) = empty_grids();

        let building = polygon![
            (x: oe + 5.0,  y: on + 5.0),
            (x: oe + 15.0, y: on + 5.0),
            (x: oe + 15.0, y: on + 15.0),
            (x: oe + 5.0,  y: on + 15.0),
            (x: oe + 5.0,  y: on + 5.0),
        ];
        // No land polys → all pixels would be water without building precedence
        let grid = build_class_grid(
            &height, &veg,
            &[&building], &[], &[], &[],
            las_id(),
        );

        assert_eq!(grid[10 * GRID_SIDE + 10], PixelClass::Building as u8);
    }

    #[test]
    fn vegetation_takes_precedence_over_water() {
        let mut height = vec![0.0; GRID_SIDE * GRID_SIDE];
        let mut veg = vec![0.0; GRID_SIDE * GRID_SIDE];

        let idx = 100 * GRID_SIDE + 100;
        height[idx] = 30.0;
        veg[idx] = 28.0;

        // No land polys → pixel would be water, but veg takes precedence
        let grid = build_class_grid(
            &height, &veg,
            &[], &[], &[], &[],
            las_id(),
        );

        assert_eq!(grid[idx], PixelClass::Vegetation as u8);
    }

    #[test]
    fn interior_pixel_with_nothing_is_none() {
        let (height, veg) = empty_grids();

        let land = whole_tile_land_poly(las_id());
        let grid = build_class_grid(
            &height, &veg,
            &[], &[], &[], &filter(&land),
            las_id(),
        );

        assert!(grid.iter().all(|&c| c == PixelClass::None as u8));
    }

    #[test]
    fn bboxes_overlap_detects_adjacent_as_non_overlapping() {
        let a = Rect::new(geo::coord! {x: 0.0, y: 0.0}, geo::coord! {x: 10.0, y: 10.0});
        let b = Rect::new(geo::coord! {x: 10.0, y: 0.0}, geo::coord! {x: 20.0, y: 10.0});
        assert!(!bboxes_overlap(&a, &b));
    }

    #[test]
    fn bboxes_overlap_detects_overlapping() {
        let a = Rect::new(geo::coord! {x: 0.0, y: 0.0}, geo::coord! {x: 10.0, y: 10.0});
        let b = Rect::new(geo::coord! {x: 5.0, y: 5.0}, geo::coord! {x: 15.0, y: 15.0});
        assert!(bboxes_overlap(&a, &b));
    }

    #[test]
    fn filter_polys_for_tile_keeps_intersecting() {
        let (oe, on) = origin();
        let side = GRID_SIDE as f64;
        let tbbox = Rect::new(
            geo::coord! {x: oe, y: on},
            geo::coord! {x: oe + side, y: on + side},
        );
        let inside = polygon![
            (x: oe + 100.0, y: on + 100.0),
            (x: oe + 110.0, y: on + 100.0),
            (x: oe + 110.0, y: on + 110.0),
            (x: oe + 100.0, y: on + 110.0),
            (x: oe + 100.0, y: on + 100.0),
        ];
        let outside = polygon![
            (x: oe + side + 100.0, y: on),
            (x: oe + side + 110.0, y: on),
            (x: oe + side + 110.0, y: on + 10.0),
            (x: oe + side + 100.0, y: on + 10.0),
            (x: oe + side + 100.0, y: on),
        ];
        let all: Vec<(Rect<f64>, Polygon<f64>)> = vec![
            (inside.bounding_rect().unwrap(), inside),
            (outside.bounding_rect().unwrap(), outside),
        ];
        let filtered = filter_polys_for_tile(&all, tbbox);
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn no_land_no_hydro_has_right_no_land() {
        let all = no_land();
        assert!(all.is_empty());
    }
}
