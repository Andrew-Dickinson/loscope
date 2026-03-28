"""Tests for los_analyzer.sample_points.perimeter"""
import numpy as np
import pytest
from shapely.geometry import Polygon

from los_analyzer.lib.sample_points.perimeter import sample_perimeter


def flat_hm(W, H, inches=120):
    return np.full((W, H), inches, dtype=np.uint16)


# ── point count ───────────────────────────────────────────────────────────────

def test_square_perimeter_point_count():
    # 20×20 square → perimeter = 80 ft; spacing=10 → np.arange(0,80,10) = 8 pts
    poly = Polygon([(0, 0), (20, 0), (20, 20), (0, 20)])
    hm = flat_hm(25, 25)
    pts = sample_perimeter(poly, hm, 0, 0, 10)
    assert len(pts) == 8


def test_single_edge_spacing():
    # 5×5 square → perimeter=20; spacing=5 → 4 pts
    poly = Polygon([(0, 0), (5, 0), (5, 5), (0, 5)])
    hm = flat_hm(10, 10)
    pts = sample_perimeter(poly, hm, 0, 0, 5)
    assert len(pts) == 4


# ── polygon with hole ─────────────────────────────────────────────────────────

def test_hole_ring_is_sampled():
    # Outer 20×20, inner 4×4 hole
    outer = [(0, 0), (20, 0), (20, 20), (0, 20)]
    inner = [(8, 8), (12, 8), (12, 12), (8, 12)]  # 4×4 hole, perimeter=16
    poly = Polygon(outer, [inner])
    hm = flat_hm(25, 25)
    pts = sample_perimeter(poly, hm, 0, 0, 10)
    # outer: np.arange(0,80,10)=8; inner: np.arange(0,16,10)=2  → total 10
    assert len(pts) == 10


# ── Z lookup ──────────────────────────────────────────────────────────────────

def test_z_from_heightmap():
    # Place the polygon entirely within pixel (0,0) so every perimeter point
    # samples the same pixel.  Pixel (0,0) covers [0,1)×[0,1).
    # Use a small triangle whose vertices and edges stay in [0.1, 0.9].
    hm = np.zeros((10, 10), dtype=np.uint16)
    hm[0, 0] = 240  # 20 ft
    poly = Polygon([(0.1, 0.1), (0.9, 0.1), (0.5, 0.9)])
    # perimeter ≈ 1.75 ft; spacing=0.5 → ~3 pts, all inside pixel (0,0)
    pts = sample_perimeter(poly, hm, 0, 0, 1)
    assert len(pts) >= 1
    assert all(np.isclose(pts[:, 2], 20.0))


def test_xy_offset_in_z_lookup():
    hm = np.zeros((10, 10), dtype=np.uint16)
    hm[3, 4] = 60  # 5 ft
    # Polygon corner at (3, 4) → floor(3-0)=3, floor(4-0)=4 → pixel (3,4)
    poly = Polygon([(3, 4), (4, 4), (4, 5), (3, 5)])
    pts = sample_perimeter(poly, hm, 0, 0, 1)
    assert any(np.isclose(pts[:, 2], 5.0))


# ── output shape ─────────────────────────────────────────────────────────────

def test_returns_float64():
    poly = Polygon([(0, 0), (5, 0), (5, 5), (0, 5)])
    pts = sample_perimeter(poly, flat_hm(10, 10), 0, 0, 2)
    assert pts.dtype == np.float64


def test_empty_degenerate_ring():
    # A polygon where the ring has zero length should not crash
    poly = Polygon([(0, 0), (0, 0), (0, 0)])
    pts = sample_perimeter(poly, flat_hm(5, 5), 0, 0, 1)
    assert pts.shape[1] == 3


# ── mask-aware sampling ───────────────────────────────────────────────────────

def test_mask_skips_point_with_no_valid_neighbour():
    """A perimeter point whose pixel and all neighbours are outside the mask should be dropped."""
    hm = np.full((10, 10), 120, dtype=np.uint16)
    mask = np.zeros((10, 10), dtype=np.uint8)
    # Only pixel (0, 0) is inside the mask; polygon is at the far corner (8-9, 8-9),
    # so its perimeter pixels and all their ±1 neighbours are outside the mask.
    mask[0, 0] = 255
    poly = Polygon([(8, 8), (9, 8), (9, 9), (8, 9)])
    pts = sample_perimeter(poly, hm, 0, 0, 1, mask=mask)
    assert len(pts) == 0


def test_mask_relocates_to_neighbour():
    """A perimeter point whose pixel is outside the mask but has a valid neighbour keeps a non-zero Z."""
    W, H = 10, 10
    hm = np.zeros((W, H), dtype=np.uint16)
    hm[1, 1] = 240  # 20 ft — the valid neighbour
    mask = np.zeros((W, H), dtype=np.uint8)
    mask[1, 1] = 255  # only this pixel is inside
    # Ring passes through pixel (0, 0) which is outside the mask; neighbour (1,1) is inside.
    poly = Polygon([(0.1, 0.1), (0.9, 0.1), (0.5, 0.9)])
    pts = sample_perimeter(poly, hm, 0, 0, 1, mask=mask)
    # At least one point should survive (relocated to pixel (1,1)), with z=20 ft.
    assert len(pts) >= 1
    assert all(np.isclose(pts[:, 2], 20.0))
