import math

import numpy as np
from pyproj import Transformer
from scipy.spatial.transform import Rotation

_GEO_TO_ECEF = Transformer.from_crs("EPSG:4979", "EPSG:4978", always_xy=True)
_ECEF_TO_GEO = Transformer.from_crs("EPSG:4978", "EPSG:4979", always_xy=True)
SPEED_OF_LIGHT = 299_792_458.0
_LAT_SEGMENTS = 24
_LON_SEGMENTS = 48


def _geo_to_ecef(lon, lat, alt):
    x, y, z = _GEO_TO_ECEF.transform(lon, lat, alt)
    return np.array([x, y, z])


def _ecef_to_geo(xyz):
    return _ECEF_TO_GEO.transform(xyz[0], xyz[1], xyz[2])


def _enu_rotation_matrix(lon_deg, lat_deg):
    """Columns are East, North, Up unit vectors in ECEF."""
    lon = math.radians(lon_deg)
    lat = math.radians(lat_deg)
    sl, cl = math.sin(lon), math.cos(lon)
    sp, cp = math.sin(lat), math.cos(lat)
    return np.array([
        [-sl,      -sp * cl,  cp * cl],
        [ cl,      -sp * sl,  cp * sl],
        [ 0.0,      cp,       sp],
    ])


def _enu_to_ecef(enu, origin):
    lon, lat, alt = origin
    return _geo_to_ecef(lon, lat, alt) + _enu_rotation_matrix(lon, lat) @ enu


def _ecef_to_enu(xyz, origin):
    lon, lat, alt = origin
    return _enu_rotation_matrix(lon, lat).T @ (xyz - _geo_to_ecef(lon, lat, alt))


def _rotation_align_z_to_los(start_wgs84, end_wgs84, origin_wgs84):
    """3×3 rotation matrix that maps the local +Z axis onto the LOS direction."""
    start_enu = _ecef_to_enu(_geo_to_ecef(*start_wgs84), origin_wgs84)
    end_enu   = _ecef_to_enu(_geo_to_ecef(*end_wgs84),   origin_wgs84)
    d = end_enu - start_enu
    d = d / np.linalg.norm(d)
    z = np.array([0.0, 0.0, 1.0])
    if np.allclose(d, z):
        return np.eye(3)
    if np.allclose(d, -z):
        return Rotation.from_euler('x', 180, degrees=True).as_matrix()
    axis = np.cross(z, d)
    axis = axis / np.linalg.norm(axis)
    angle = float(np.arccos(np.clip(np.dot(z, d), -1.0, 1.0)))
    return Rotation.from_rotvec(angle * axis).as_matrix()


def build_fresnel_kml(analysis_id: str, start_wgs84, end_wgs84, frequency_hz: float) -> str:
    start_ecef = _geo_to_ecef(*start_wgs84)
    end_ecef   = _geo_to_ecef(*end_wgs84)
    distance   = float(np.linalg.norm(end_ecef - start_ecef))

    wavelength   = SPEED_OF_LIGHT / frequency_hz
    semi_major   = distance / 2.0 + wavelength / 4.0
    semi_minor   = math.sqrt(semi_major ** 2 - (distance / 2.0) ** 2)
    center_wgs84 = _ecef_to_geo((start_ecef + end_ecef) / 2.0)

    polygons = _ellipsoid_polygons(start_wgs84, end_wgs84, center_wgs84, semi_major, semi_minor)
    return _kml_string(analysis_id, polygons, start_wgs84, end_wgs84)


def _ellipsoid_polygons(start_wgs84, end_wgs84, center_wgs84, semi_major, semi_minor):
    """Return quad polygons (each a list of (lon, lat, alt)) approximating the ellipsoid."""
    a, b = semi_major, semi_minor
    R = _rotation_align_z_to_los(start_wgs84, end_wgs84, center_wgs84)

    verts_geo = []
    for i in range(_LAT_SEGMENTS + 1):
        theta = math.pi * i / _LAT_SEGMENTS
        for j in range(_LON_SEGMENTS):
            phi = 2.0 * math.pi * j / _LON_SEGMENTS
            local = np.array([
                b * math.sin(theta) * math.cos(phi),
                b * math.sin(theta) * math.sin(phi),
                a * math.cos(theta),
            ])
            verts_geo.append(_ecef_to_geo(_enu_to_ecef(R @ local, center_wgs84)))

    L = _LON_SEGMENTS
    polygons = []
    for i in range(_LAT_SEGMENTS):
        for j in range(L):
            i1 = i * L + j
            i2 = i * L + (j + 1) % L
            i3 = (i + 1) * L + (j + 1) % L
            i4 = (i + 1) * L + j
            polygons.append([verts_geo[i1], verts_geo[i4], verts_geo[i3], verts_geo[i2]])

    return polygons


def _kml_string(analysis_id, polygons, start_wgs84, end_wgs84):
    sa, ea = start_wgs84, end_wgs84
    los_coords = (f'{sa[0]:.8f},{sa[1]:.8f},{sa[2]:.3f} '
                  f'{ea[0]:.8f},{ea[1]:.8f},{ea[2]:.3f}')
    lines = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        '<kml xmlns="http://www.opengis.net/kml/2.2">',
        '<Document>',
        f'  <name>{analysis_id}</name>',
        '  <Style id="fresnel">',
        '    <PolyStyle>',
        '      <color>99ff44cc</color>',
        '      <outline>0</outline>',
        '    </PolyStyle>',
        '  </Style>',
        '  <Style id="los">',
        '    <LineStyle>',
        '      <color>ffaa2277</color>',
        '      <width>3</width>',
        '    </LineStyle>',
        '  </Style>',
        '  <Placemark>',
        '    <styleUrl>#los</styleUrl>',
        '    <LineString>',
        '      <altitudeMode>absolute</altitudeMode>',
        f'      <coordinates>{los_coords}</coordinates>',
        '    </LineString>',
        '  </Placemark>',
        '  <Placemark>',
        '    <styleUrl>#fresnel</styleUrl>',
        '    <MultiGeometry>',
    ]
    for poly in polygons:
        first = poly[0]
        coords = ' '.join(f'{lon:.8f},{lat:.8f},{alt:.3f}' for lon, lat, alt in poly)
        coords += f' {first[0]:.8f},{first[1]:.8f},{first[2]:.3f}'
        lines += [
            '      <Polygon>',
            '        <altitudeMode>absolute</altitudeMode>',
            '        <outerBoundaryIs><LinearRing>',
            f'          <coordinates>{coords}</coordinates>',
            '        </LinearRing></outerBoundaryIs>',
            '      </Polygon>',
        ]
    lines += [
        '    </MultiGeometry>',
        '  </Placemark>',
        '</Document>',
        '</kml>',
    ]
    return '\n'.join(lines)
