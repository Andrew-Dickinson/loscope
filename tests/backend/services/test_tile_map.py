"""Tests for los_analyzer.backend.services.tile_map"""
import numpy as np
import pytest
from PIL import Image

# Bootstrap the full app import chain before importing individual services.
from los_analyzer.backend.app import app as _app  # noqa: F401
from los_analyzer.backend.services.tile_map import (
    fresnel_ellipse_ring,
    rasterize_intersection_grid_for_tile,
    intersection_image_for_tile,
)
from los_analyzer.lib.tiles.intersect import IntersectionGrid

# tile "235_00": e0=1000000, n0=235000
_TILE_ID = "235_00"
_NYS_A = (982000.0, 200000.0)
_NYS_B = (983000.0, 200000.0)
_FREQ_HZ = 5.8e9


def _make_grid(H=10, maxW=50, value=0.5, x_base=1000050, y_base=235100):
    return IntersectionGrid(
        values=np.full((H, maxW), value, dtype=np.float32),
        widths=np.full(H, maxW, dtype=np.uint32),
        offsets=np.zeros(H, dtype=np.uint32),
        x_base_offset=x_base,
        y_base_offset=y_base,
    )


# ---------------------------------------------------------------------------
# fresnel_ellipse_ring
# ---------------------------------------------------------------------------

class TestFresnelEllipseRing:
    def test_returns_n_pts_plus_one_points(self):
        ring = fresnel_ellipse_ring(_NYS_A, _NYS_B, _FREQ_HZ, n_pts=90)
        assert len(ring) == 91

    def test_ring_is_closed(self):
        """First and last points should be the same (closed polygon)."""
        ring = fresnel_ellipse_ring(_NYS_A, _NYS_B, _FREQ_HZ, n_pts=90)
        assert ring[0] == ring[-1]

    def test_zero_length_line_returns_empty(self):
        ring = fresnel_ellipse_ring(_NYS_A, _NYS_A, _FREQ_HZ)
        assert ring == []

    def test_n_pts_parameter_controls_ring_size(self):
        ring_coarse = fresnel_ellipse_ring(_NYS_A, _NYS_B, _FREQ_HZ, n_pts=12)
        ring_fine = fresnel_ellipse_ring(_NYS_A, _NYS_B, _FREQ_HZ, n_pts=36)
        assert len(ring_coarse) == 13
        assert len(ring_fine) == 37

    def test_each_point_is_iterable_with_coords(self):
        ring = fresnel_ellipse_ring(_NYS_A, _NYS_B, _FREQ_HZ, n_pts=4)
        for pt in ring:
            assert len(pt) >= 2  # at least (x, y) or (lon, lat, elev)

    def test_alpha_zero_returns_degenerate_ring(self):
        """alpha=0 collapses the minor axis; ring points should cluster on the link axis."""
        ring = fresnel_ellipse_ring(_NYS_A, _NYS_B, _FREQ_HZ, alpha=0.0, n_pts=8)
        assert len(ring) == 9


# ---------------------------------------------------------------------------
# rasterize_intersection_grid_for_tile
# ---------------------------------------------------------------------------

class TestRasterizeIntersectionGridForTile:
    def test_output_shape_is_tile_size(self):
        from los_analyzer.lib.preprocessing.tile_id import TILE_SIDE_USFT
        grid = _make_grid()
        result = rasterize_intersection_grid_for_tile(_TILE_ID, grid)
        assert result.shape == (TILE_SIDE_USFT, TILE_SIDE_USFT)

    def test_overlapping_grid_has_nonzero_values(self):
        grid = _make_grid(value=0.7)
        result = rasterize_intersection_grid_for_tile(_TILE_ID, grid)
        assert result.max() > 0

    def test_nonoverlapping_grid_returns_all_zeros(self):
        """A grid far outside the tile should produce an all-zero output."""
        grid = _make_grid(y_base=500000)  # far above tile "235_00"
        result = rasterize_intersection_grid_for_tile(_TILE_ID, grid)
        assert (result == 0).all()

    def test_values_clipped_to_zero_one(self):
        """IntersectionGrid values are [0,1]; rasterized output should stay in that range."""
        grid = _make_grid(value=0.5)
        result = rasterize_intersection_grid_for_tile(_TILE_ID, grid)
        assert result.min() >= 0.0
        assert result.max() <= 1.0


# ---------------------------------------------------------------------------
# intersection_image_for_tile
# ---------------------------------------------------------------------------

class TestIntersectionImageForTile:
    def test_returns_pil_image_for_overlapping_grid(self):
        grid = _make_grid(value=0.5)
        img = intersection_image_for_tile(_TILE_ID, grid)
        assert isinstance(img, Image.Image)

    def test_returns_none_for_all_zero_grid(self):
        grid = _make_grid(value=0.0)
        img = intersection_image_for_tile(_TILE_ID, grid)
        assert img is None

    def test_image_mode_is_rgba(self):
        grid = _make_grid(value=0.5)
        img = intersection_image_for_tile(_TILE_ID, grid)
        assert img.mode == "RGBA"

    def test_image_size_matches_tile(self):
        from los_analyzer.lib.preprocessing.tile_id import TILE_SIDE_USFT
        grid = _make_grid(value=0.5)
        img = intersection_image_for_tile(_TILE_ID, grid)
        assert img.size == (TILE_SIDE_USFT, TILE_SIDE_USFT)
