"""Tests for los_analyzer.sample_points.grid"""
import numpy as np
import pytest

from lib.sample_points.grid import sample_grid


# ── helpers ──────────────────────────────────────────────────────────────────

def flat_hm(W, H, inches=120):
    """Uniform-height uint16 heightmap (default 120 in = 10 ft)."""
    return np.full((W, H), inches, dtype=np.uint16)


# ── basic grid geometry ───────────────────────────────────────────────────────

def test_invalid_spacing():
    with pytest.raises(ValueError):
        sample_grid(flat_hm(4, 4), 0, 0, 0)


def test_spacing_larger_than_heightmap_returns_single_point():
    # spacing=10 on a 4×4 grid → arange(5, 4, 10) is empty → no points
    hm = flat_hm(4, 4)
    base, cliff = sample_grid(hm, 0, 0, 10)
    assert base.shape == (0, 3)
    assert cliff.shape == (0, 3)


def test_spacing_1_samples_every_pixel():
    hm = flat_hm(3, 3, inches=60)  # 5 ft
    base, cliff = sample_grid(hm, 0, 0, 1)
    assert base.shape == (9, 3)
    assert cliff.shape == (0, 3)
    assert np.allclose(base[:, 2], 5.0)


def test_grid_xy_positions_spacing_2():
    # 4×4 heightmap, spacing=2 → sample pixel indices 1, 3 in each axis
    hm = flat_hm(4, 4)
    base, cliff = sample_grid(hm, 10, 20, 2)
    assert base.shape == (4, 3)
    expected_x = sorted([10 + 1 + 0.5, 10 + 3 + 0.5] * 2)
    expected_y = sorted([20 + 1 + 0.5, 20 + 3 + 0.5] * 2)
    assert sorted(base[:, 0].tolist()) == pytest.approx(expected_x)
    assert sorted(base[:, 1].tolist()) == pytest.approx(expected_y)


def test_xy_offset_applied():
    hm = flat_hm(2, 2)
    base, _ = sample_grid(hm, 100, 200, 1)
    # Pixel indices 0 and 1; world coords 100.5, 101.5 and 200.5, 201.5
    assert set(np.round(base[:, 0], 6).tolist()) == {100.5, 101.5}
    assert set(np.round(base[:, 1], 6).tolist()) == {200.5, 201.5}


# ── mask filtering ────────────────────────────────────────────────────────────

def test_mask_zero_suppresses_all():
    hm = flat_hm(3, 3)
    mask = np.zeros((3, 3), dtype=np.uint8)
    base, cliff = sample_grid(hm, 0, 0, 1, mask=mask)
    assert base.shape == (0, 3)
    assert cliff.shape == (0, 3)


def test_mask_filters_to_inside_pixels():
    hm = flat_hm(3, 3, inches=240)
    mask = np.zeros((3, 3), dtype=np.uint8)
    mask[1, 1] = 255  # only centre pixel inside
    base, cliff = sample_grid(hm, 0, 0, 1, mask=mask)
    assert base.shape == (1, 3)
    assert np.isclose(base[0, 0], 1.5)
    assert np.isclose(base[0, 1], 1.5)
    assert np.isclose(base[0, 2], 20.0)  # 240 in / 12 = 20 ft


def test_mask_none_includes_all():
    hm = flat_hm(2, 2)
    base, _ = sample_grid(hm, 0, 0, 1, mask=None)
    assert base.shape == (4, 3)


# ── cliff compensation ────────────────────────────────────────────────────────

def test_no_cliff_when_diff_at_threshold():
    # diff exactly == step_in → not strictly greater → no cliff
    # spacing=1 → step_in=12 in. Make neighbor exactly 12 in higher.
    hm = np.array([[0, 12]], dtype=np.uint16)  # [0,0]=0 in, [1,0]=12 in
    base, cliff = sample_grid(hm, 0, 0, 1)
    assert cliff.shape == (0, 3)


def test_cliff_triggered_when_diff_exceeds_threshold():
    # trigger=spacing*12=12 in. diff=13 > 12 → cliff fires.
    # cliff_step=spacing*6=6 in. n_extra=int(13/6)=2; + 1 cap = 3 points.
    # z: [6,12,18] in → [0.5, 1.0, 1.5] ft.
    hm = np.array([[0, 13]], dtype=np.uint16)
    base, cliff = sample_grid(hm, 0, 0, 1)
    at_00 = cliff[np.isclose(cliff[:, 0], 0.5) & np.isclose(cliff[:, 1], 0.5)]
    assert len(at_00) == 3
    assert np.allclose(at_00[:, 2], [0.5, 1.0, 1.5])


def test_cliff_multiple_extra_points():
    # trigger=12 in. diff=37 > 12 → cliff fires.
    # cliff_step=6 in. n_extra=int(37/6)=6; + 1 cap = 7 points.
    # z: [6,12,18,24,30,36,42] in → [0.5,1.0,1.5,2.0,2.5,3.0,3.5] ft.
    hm = np.array([[0, 37]], dtype=np.uint16)
    base, cliff = sample_grid(hm, 0, 0, 1)
    at_00 = cliff[np.isclose(cliff[:, 0], 0.5) & np.isclose(cliff[:, 1], 0.5)]
    assert len(at_00) == 7
    assert np.allclose(at_00[:, 2], [0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5])


def test_cliff_cap_one_half_step_above_last():
    # trigger=12 in. diff=24 > 12 → cliff fires.
    # cliff_step=6 in. n_extra=int(24/6)=4; + 1 cap = 5 points.
    # z: [6,12,18,24,30] in → [0.5, 1.0, 1.5, 2.0, 2.5] ft.
    hm = np.array([[0, 24]], dtype=np.uint16)
    base, cliff = sample_grid(hm, 0, 0, 1)
    at_00 = cliff[np.isclose(cliff[:, 0], 0.5) & np.isclose(cliff[:, 1], 0.5)]
    assert len(at_00) == 5
    assert np.allclose(at_00[:, 2], [0.5, 1.0, 1.5, 2.0, 2.5])


def test_cliff_only_for_inside_pixels():
    # pixel [0,0] is outside mask; [1,0] has a tall neighbour.
    # Only [1,0] should generate cliff points.
    hm = np.array([[0, 50], [0, 50]], dtype=np.uint16)
    mask = np.array([[0, 255], [255, 255]], dtype=np.uint8)
    _, cliff = sample_grid(hm, 0, 0, 1, mask=mask)
    # [0,1]: h=50, neighbour [0,0] h=0 → neighbour is LOWER, no cliff for [0,1]
    # [1,0]: h=0, neighbour [1,1] h=50 → diff=50>12 → cliff. Also [0,0] masked out.
    # [1,1]: h=50, neighbour [1,0] h=0 → lower, no cliff
    # So only [1,0] produces cliff points
    assert all(np.isclose(cliff[:, 0], 1.5))


def test_cliff_z_values_are_feet():
    # Ensure output z is in feet not inches.
    # trigger=12 in. diff=120 > 12 → cliff fires.
    # cliff_step=6 in. n_extra=int(120/6)=20; + 1 cap = 21 points.
    # z: [6,12,...,126] in → [0.5, 1.0, ..., 10.5] ft.
    hm = np.array([[0, 120]], dtype=np.uint16)  # neighbour = 10 ft = 120 in
    _, cliff = sample_grid(hm, 0, 0, 1)
    at_00 = cliff[np.isclose(cliff[:, 0], 0.5)]
    assert len(at_00) == 21
    assert np.allclose(at_00[:, 2], np.arange(0.5, 11.0, 0.5))
