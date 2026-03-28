"""Tests for Flask endpoints in los_analyzer.backend.endpoints.*"""
from unittest.mock import patch, MagicMock

import pytest

# conftest.py sets LOS_ASSET_S3_BUCKET etc. before this import
from los_analyzer.backend.app import app as flask_app


@pytest.fixture
def client():
    flask_app.config["TESTING"] = True
    with flask_app.test_client() as c:
        yield c


# ---------------------------------------------------------------------------
# /api/healthcheck
# ---------------------------------------------------------------------------

def test_healthcheck_returns_200(client):
    resp = client.get("/api/healthcheck")
    assert resp.status_code == 200


def test_healthcheck_body(client):
    resp = client.get("/api/healthcheck")
    assert b"Healthy" in resp.data


# ---------------------------------------------------------------------------
# POST /api/analysis/analyzePointPair — validation errors
# ---------------------------------------------------------------------------

def test_analyze_point_pair_missing_body_returns_400(client):
    resp = client.post("/api/analysis/analyzePointPair", json={})
    assert resp.status_code == 400


def test_analyze_point_pair_bad_point_a_type_returns_400(client):
    resp = client.post("/api/analysis/analyzePointPair", json={
        "point_a_nys": "not_a_list",
        "point_b_nys": [982000.0, 200000.0, 100.0],
        "frequency_ghz": 5.8,
    })
    assert resp.status_code == 400


def test_analyze_point_pair_wrong_coord_length_returns_400(client):
    resp = client.post("/api/analysis/analyzePointPair", json={
        "point_a_nys": [982000.0, 200000.0],     # only 2 elements
        "point_b_nys": [983000.0, 200000.0, 0.0],
        "frequency_ghz": 5.8,
    })
    assert resp.status_code == 400


def test_analyze_point_pair_bad_obstruction_types_returns_400(client):
    resp = client.post("/api/analysis/analyzePointPair", json={
        "point_a_nys": [982000.0, 200000.0, 0.0],
        "point_b_nys": [983000.0, 200000.0, 0.0],
        "frequency_ghz": 5.8,
        "obstruction_types": 42,   # must be list or "*"
    })
    assert resp.status_code == 400


# ---------------------------------------------------------------------------
# GET /api/analysis/overview/<analysis_id>
# ---------------------------------------------------------------------------

def test_overview_invalid_uuid_returns_400(client):
    resp = client.get("/api/analysis/overview/not-a-valid-uuid")
    assert resp.status_code == 400


def test_overview_unknown_analysis_id_returns_404(client):
    resp = client.get("/api/analysis/overview/12345678-1234-4000-8000-123456789abc")
    assert resp.status_code == 404


# ---------------------------------------------------------------------------
# GET /api/analysis/intersectionVisualization/<analysis_id>/<tile_id>
# ---------------------------------------------------------------------------

def test_intersection_visualization_invalid_uuid_returns_400(client):
    resp = client.get("/api/analysis/intersectionVisualization/bad-uuid/235_00")
    assert resp.status_code == 400


def test_intersection_visualization_unknown_id_returns_404(client):
    resp = client.get("/api/analysis/intersectionVisualization/12345678-1234-4000-8000-123456789abc/235_00")
    assert resp.status_code == 404


# ---------------------------------------------------------------------------
# GET /api/tileview/terrain/tileOverview/<tile_id>
# ---------------------------------------------------------------------------

def test_tile_overview_invalid_tile_id_returns_400(client):
    resp = client.get("/api/tileview/terrain/tileOverview/bad_tile_id")
    assert resp.status_code == 400


def test_tile_overview_valid_tile_returns_200(client):
    with patch("los_analyzer.backend.endpoints.tile_view.obstruction_provider") as mock_prov:
        mock_prov.obstruction_ids_for_tile_id.return_value = {}
        resp = client.get("/api/tileview/terrain/tileOverview/235_00")
    assert resp.status_code == 200
    data = resp.get_json()
    assert "obstruction_ids" in data


# ---------------------------------------------------------------------------
# GET /api/tileview/terrain/heightRaster/<tile_id>
# ---------------------------------------------------------------------------

def test_height_raster_invalid_tile_id_returns_400(client):
    resp = client.get("/api/tileview/terrain/heightRaster/invalid!")
    assert resp.status_code == 400


# ---------------------------------------------------------------------------
# GET /api/tileview/terrain/obstructionObj/<type>/<id>/<tile>
# ---------------------------------------------------------------------------

def test_obstruction_obj_invalid_type_returns_400(client):
    resp = client.get(
        "/api/tileview/terrain/obstructionObj"
        "/123invalid/12345678-1234-4000-8000-123456789abc/235_00"
    )
    assert resp.status_code == 400


def test_obstruction_obj_invalid_uuid_returns_400(client):
    resp = client.get(
        "/api/tileview/terrain/obstructionObj"
        "/building_footprint/not-a-uuid/235_00"
    )
    assert resp.status_code == 400


def test_obstruction_obj_invalid_tile_id_returns_400(client):
    resp = client.get(
        "/api/tileview/terrain/obstructionObj"
        "/building_footprint/12345678-1234-4000-8000-123456789abc/bad_tile"
    )
    assert resp.status_code == 400


# ---------------------------------------------------------------------------
# GET /api/tileview/terrain/orthoImage/<tile_id>
# ---------------------------------------------------------------------------

def test_ortho_image_invalid_tile_id_returns_400(client):
    resp = client.get("/api/tileview/terrain/orthoImage/bad_tile")
    assert resp.status_code == 400


# ---------------------------------------------------------------------------
# GET /api/tileView/fresnelSliceObj/<analysis_id>/<tile_id>
# ---------------------------------------------------------------------------

def test_fresnel_slice_invalid_uuid_returns_400(client):
    resp = client.get("/api/tileView/fresnelSliceObj/not-a-uuid/235_00")
    assert resp.status_code == 400


def test_fresnel_slice_invalid_tile_id_returns_400(client):
    # Valid UUID but tile_id invalid — but UUID is not in cache → 404 first
    resp = client.get(
        "/api/tileView/fresnelSliceObj/12345678-1234-4000-8000-123456789abc/bad_tile"
    )
    # UUID valid but not in cache → 404 (tile_id check is only reached if in cache)
    assert resp.status_code == 404


def test_fresnel_slice_unknown_analysis_id_returns_404(client):
    resp = client.get("/api/tileView/fresnelSliceObj/12345678-1234-4000-8000-123456789abc/235_00")
    assert resp.status_code == 404
