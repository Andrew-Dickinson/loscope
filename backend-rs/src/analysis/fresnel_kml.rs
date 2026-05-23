use std::collections::HashMap;
use std::f64::consts::PI;
use uuid::Uuid;
use nalgebra::{Matrix3, Vector3};
use eproj::{Coordinate3, Projector, SpatialReferenceIdentifier};
use kml::{Kml, KmlDocument, KmlVersion};
use kml::types::{
    AltitudeMode, Coord, Element, Geometry, LineString, LinearRing,
    LineStyle, MultiGeometry, Placemark, Polygon, PolyStyle, Style,
};
use crate::types::coords::GPSCoords3;

const SPEED_OF_LIGHT: f64 = 299_792_458.0;
const LAT_SEGMENTS: usize = 24;
const LON_SEGMENTS: usize = 48;

// (lon, lat, alt_m) - matching pyproj always_xy=True convention
type Wgs84 = (f64, f64, f64);

fn make_geo_to_ecef() -> Projector {
    Projector::new(SpatialReferenceIdentifier::Epsg4979, SpatialReferenceIdentifier::Epsg4978).unwrap()
}

fn make_ecef_to_geo() -> Projector {
    Projector::new(SpatialReferenceIdentifier::Epsg4978, SpatialReferenceIdentifier::Epsg4979).unwrap()
}

fn geo_to_ecef(proj: &Projector, lon: f64, lat: f64, alt: f64) -> Vector3<f64> {
    proj.convert(Coordinate3::new(lon, lat, alt)).unwrap().into()
}

fn ecef_to_geo(proj: &Projector, xyz: Vector3<f64>) -> Wgs84 {
    let r = proj.convert(Coordinate3::from(xyz)).unwrap();
    (r.x(), r.y(), r.z())
}

fn enu_rotation_matrix(lon_deg: f64, lat_deg: f64) -> Matrix3<f64> {
    let lon = lon_deg.to_radians();
    let lat = lat_deg.to_radians();
    let (sl, cl) = (lon.sin(), lon.cos());
    let (sp, cp) = (lat.sin(), lat.cos());
    Matrix3::new(
        -sl,   -sp * cl,  cp * cl,
         cl,   -sp * sl,  cp * sl,
         0.0,   cp,       sp,
    )
}

fn enu_to_ecef(g2e: &Projector, enu: Vector3<f64>, origin: Wgs84) -> Vector3<f64> {
    let (lon, lat, alt) = origin;
    geo_to_ecef(g2e, lon, lat, alt) + enu_rotation_matrix(lon, lat) * enu
}

fn ecef_to_enu(g2e: &Projector, xyz: Vector3<f64>, origin: Wgs84) -> Vector3<f64> {
    let (lon, lat, alt) = origin;
    enu_rotation_matrix(lon, lat).transpose() * (xyz - geo_to_ecef(g2e, lon, lat, alt))
}

fn rotation_align_z_to_los(g2e: &Projector, start: Wgs84, end: Wgs84, origin: Wgs84) -> Matrix3<f64> {
    let start_enu = ecef_to_enu(g2e, geo_to_ecef(g2e, start.0, start.1, start.2), origin);
    let end_enu   = ecef_to_enu(g2e, geo_to_ecef(g2e, end.0,   end.1,   end.2),   origin);
    let d_raw = end_enu - start_enu;
    let d = d_raw / d_raw.norm();
    let z = Vector3::new(0.0, 0.0, 1.0);
    if (d - z).norm() < 1e-9 {
        return Matrix3::identity();
    }
    if (d + z).norm() < 1e-9 {
        return Matrix3::new(1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, -1.0);
    }
    let axis_raw = z.cross(&d);
    let axis = axis_raw / axis_raw.norm();
    let angle = z.dot(&d).clamp(-1.0, 1.0).acos();
    let (c, s) = (angle.cos(), angle.sin());
    let (kx, ky, kz) = (axis.x, axis.y, axis.z);
    Matrix3::new(
        c + kx*kx*(1.0-c),       kx*ky*(1.0-c) - kz*s,  kx*kz*(1.0-c) + ky*s,
        ky*kx*(1.0-c) + kz*s,   c + ky*ky*(1.0-c),      ky*kz*(1.0-c) - kx*s,
        kz*kx*(1.0-c) - ky*s,   kz*ky*(1.0-c) + kx*s,  c + kz*kz*(1.0-c),
    )
}

fn ellipsoid_polygons(
    g2e: &Projector,
    e2g: &Projector,
    start: Wgs84,
    end: Wgs84,
    center: Wgs84,
    semi_major: f64,
    semi_minor: f64,
) -> Vec<Vec<Wgs84>> {
    let r = rotation_align_z_to_los(g2e, start, end, center);

    let mut verts = Vec::with_capacity((LAT_SEGMENTS + 1) * LON_SEGMENTS);
    for i in 0..=LAT_SEGMENTS {
        let theta = PI * i as f64 / LAT_SEGMENTS as f64;
        for j in 0..LON_SEGMENTS {
            let phi = 2.0 * PI * j as f64 / LON_SEGMENTS as f64;
            let local = Vector3::new(
                semi_minor * theta.sin() * phi.cos(),
                semi_minor * theta.sin() * phi.sin(),
                semi_major * theta.cos(),
            );
            verts.push(ecef_to_geo(e2g, enu_to_ecef(g2e, r * local, center)));
        }
    }

    let l = LON_SEGMENTS;
    let mut polygons = Vec::with_capacity(LAT_SEGMENTS * l);
    for i in 0..LAT_SEGMENTS {
        for j in 0..l {
            let i1 = i * l + j;
            let i2 = i * l + (j + 1) % l;
            let i3 = (i + 1) * l + (j + 1) % l;
            let i4 = (i + 1) * l + j;
            polygons.push(vec![verts[i1], verts[i4], verts[i3], verts[i2]]);
        }
    }
    polygons
}

pub fn build_fresnel_kml(analysis_id: &Uuid, start: GPSCoords3, end: GPSCoords3, frequency_hz: f64) -> Kml {
    let g2e = make_geo_to_ecef();
    let e2g = make_ecef_to_geo();

    let start_wgs84: Wgs84 = (*start.lon(), *start.lat(), *start.alt_m());
    let end_wgs84: Wgs84 = (*end.lon(), *end.lat(), *end.alt_m());

    let start_ecef = geo_to_ecef(&g2e, start_wgs84.0, start_wgs84.1, start_wgs84.2);
    let end_ecef   = geo_to_ecef(&g2e, end_wgs84.0,   end_wgs84.1,   end_wgs84.2);
    let distance   = (end_ecef - start_ecef).norm();

    let wavelength = SPEED_OF_LIGHT / frequency_hz;
    let semi_major = distance / 2.0 + wavelength / 4.0;
    let semi_minor = (semi_major * semi_major - (distance / 2.0).powi(2)).sqrt();
    let center_wgs84 = ecef_to_geo(&e2g, (start_ecef + end_ecef) / 2.0);

    let polygons = ellipsoid_polygons(
        &g2e, &e2g, start_wgs84, end_wgs84, center_wgs84, semi_major, semi_minor,
    );

    let fresnel_style = Kml::Style(Style {
        id: Some("fresnel".to_string()),
        poly: Some(PolyStyle {
            color: "99ff44cc".to_string(),
            outline: false,
            ..Default::default()
        }),
        ..Default::default()
    });

    let los_style = Kml::Style(Style {
        id: Some("los".to_string()),
        line: Some(LineStyle {
            color: "ffaa2277".to_string(),
            width: 3.0,
            ..Default::default()
        }),
        ..Default::default()
    });

    let los_placemark = Kml::Placemark(Placemark {
        style_url: Some("#los".to_string()),
        geometry: Some(Geometry::LineString(LineString {
            coords: vec![
                Coord::new(start_wgs84.0, start_wgs84.1, Some(start_wgs84.2)),
                Coord::new(end_wgs84.0,   end_wgs84.1,   Some(end_wgs84.2)),
            ],
            altitude_mode: AltitudeMode::Absolute,
            ..Default::default()
        })),
        ..Default::default()
    });

    let geo_polygons: Vec<Geometry> = polygons.into_iter().map(|poly| {
        let first = poly[0];
        let mut coords: Vec<Coord> = poly.iter()
            .map(|&(lon, lat, alt)| Coord::new(lon, lat, Some(alt)))
            .collect();
        coords.push(Coord::new(first.0, first.1, Some(first.2)));
        Geometry::Polygon(Polygon {
            outer: LinearRing {
                coords,
                altitude_mode: AltitudeMode::Absolute,
                ..Default::default()
            },
            altitude_mode: AltitudeMode::Absolute,
            ..Default::default()
        })
    }).collect();

    let ellipsoid_placemark = Kml::Placemark(Placemark {
        style_url: Some("#fresnel".to_string()),
        geometry: Some(Geometry::MultiGeometry(MultiGeometry::new(geo_polygons))),
        ..Default::default()
    });

    let name_el = Kml::Element(Element {
        name: "name".to_string(),
        content: Some(analysis_id.to_string()),
        ..Default::default()
    });

    Kml::KmlDocument(KmlDocument {
        version: KmlVersion::V22,
        attrs: HashMap::from([("xmlns".to_string(), "http://www.opengis.net/kml/2.2".to_string())]),
        elements: vec![
            Kml::Document {
                attrs: HashMap::new(),
                elements: vec![name_el, fresnel_style, los_style, los_placemark, ellipsoid_placemark],
            },
        ],
    })
}
