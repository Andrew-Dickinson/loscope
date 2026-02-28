from __future__ import annotations

from dataclasses import dataclass

import os
import numpy as np
import pyproj

from los_analyzer.preprocessing.tile_id import TILE_SIDE_USFT

SPEED_OF_LIGHT_M_S = 299_792_458
USFT_PER_METER = 1 / 0.3048006096


@dataclass
class FresnelZone:
    top: np.ndarray       # float64, shape (W, H), usft; NaN outside mask
    bottom: np.ndarray    # float64, shape (W, H), usft; NaN outside mask
    mask: np.ndarray      # uint8,   shape (W, H), 1=present 0=absent
    x_offset: int         # min easting  (west edge)  of grid in NYS usft
    y_offset: int         # min northing (south edge) of grid in NYS usft


def compute_fresnel_zone(
    point_a: tuple[float, float, float],
    point_b: tuple[float, float, float],
    frequency_hz: float,
    alpha: float = 1.0,
) -> FresnelZone:
    """Compute the 1st Fresnel zone ellipsoid projected onto a NYS plane raster.

    Args:
        point_a: (lat, lon, alt_m) in WGS84+EGM96 (EPSG:4326+5773).
        point_b: (lat, lon, alt_m) in WGS84+EGM96 (EPSG:4326+5773).
        frequency_hz: Radio frequency in Hz.
        alpha: Fresnel zone radius scale factor (1.0 = full zone).

    Returns:
        FresnelZone with top/bottom altitude arrays and mask on a 1-usft grid.
    """
    latA, lonA, altA_m = point_a
    latB, lonB, altB_m = point_b

    # Step 1 — Convert endpoints to NYS plane
    nys_crs = pyproj.CRS.from_string("EPSG:6539+6360")
    gps_crs = pyproj.CRS.from_string("EPSG:4326+5773")
    t_to_nys = pyproj.Transformer.from_crs(gps_crs, nys_crs, always_xy=False)
    xA, yA, zA = t_to_nys.transform(latA, lonA, altA_m)
    xB, yB, zB = t_to_nys.transform(latB, lonB, altB_m)

    # Step 2 — Compute LOS length in meters via ECEF
    ecef_crs = pyproj.CRS.from_epsg(4978)
    t_to_ecef = pyproj.Transformer.from_crs(gps_crs, ecef_crs, always_xy=False)
    A_ecef = np.array(t_to_ecef.transform(latA, lonA, altA_m))
    B_ecef = np.array(t_to_ecef.transform(latB, lonB, altB_m))
    L_m = np.linalg.norm(B_ecef - A_ecef)
    wavelength_m = SPEED_OF_LIGHT_M_S / frequency_hz

    # Step 3 — Determine grid bounds (with buffer)
    r_max_m = alpha * np.sqrt(wavelength_m * L_m / 4)
    r_max_usft = r_max_m * USFT_PER_METER

    x_inner_min = min(xA, xB) - r_max_usft
    x_inner_max = max(xA, xB) + r_max_usft
    y_inner_min = min(yA, yB) - r_max_usft
    y_inner_max = max(yA, yB) + r_max_usft

    x_offset = int(np.floor(x_inner_min)) - TILE_SIDE_USFT
    y_offset = int(np.floor(y_inner_min)) - TILE_SIDE_USFT
    x_end = int(np.ceil(x_inner_max)) + TILE_SIDE_USFT
    y_end = int(np.ceil(y_inner_max)) + TILE_SIDE_USFT
    W = x_end - x_offset
    H = y_end - y_offset

    # Step 4 — Build vectorised grid and compute fresnel zone
    XX, YY = np.meshgrid(
        np.arange(x_offset, x_end),
        np.arange(y_offset, y_end),
        indexing='ij',
    )

    dxL = xB - xA
    dyL = yB - yA
    L_usft_sq = dxL ** 2 + dyL ** 2

    t = ((XX - xA) * dxL + (YY - yA) * dyL) / L_usft_sq
    t_clamped = np.clip(t, 0.0, 1.0)

    proj_X = xA + t_clamped * dxL
    proj_Y = yA + t_clamped * dyL
    h_usft = np.sqrt((XX - proj_X) ** 2 + (YY - proj_Y) ** 2)

    d1_m = t_clamped * L_m
    d2_m = (1.0 - t_clamped) * L_m
    denom = d1_m + d2_m
    safe_denom = np.where(denom > 0, denom, 1.0)
    r_m = alpha * np.sqrt(wavelength_m * d1_m * d2_m / safe_denom)
    r_usft = r_m * USFT_PER_METER

    mask = ((t >= 0.0) & (t <= 1.0) & (h_usft <= r_usft)).astype(np.uint8)

    z_los = zA + t * (zB - zA)
    v_usft = np.sqrt(np.maximum(r_usft ** 2 - h_usft ** 2, 0.0))

    top = np.where(mask, z_los + v_usft, np.nan)
    bottom = np.where(mask, z_los - v_usft, np.nan)

    return FresnelZone(top=top, bottom=bottom, mask=mask,
                       x_offset=x_offset, y_offset=y_offset)

if __name__ == "__main__":
    print(os.getpid())
    input("Press enter key to continue...")
    compute_fresnel_zone((40.650, -73.979, 100.0), (40.7173, -74.0060, 100.0), 2400000000.0, 1.0)