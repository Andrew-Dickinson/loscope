"""MeshDB integration endpoint — resolves NN/install numbers to BINs."""
from __future__ import annotations

import os

import requests
from flask import abort, jsonify

from los_analyzer.backend.app import app

MESHDB_BASE_URL = "https://db.nycmesh.net"
_MESHDB_TOKEN = os.environ.get("MESHDB_API_TOKEN", "")

_SESSION = requests.Session()


def _meshdb_get(path: str) -> requests.Response:
    if not _MESHDB_TOKEN:
        abort(503, "MESHDB_API_TOKEN is not configured")
    return _SESSION.get(
        f"{MESHDB_BASE_URL}{path}",
        headers={"Authorization": f"Token {_MESHDB_TOKEN}"},
        timeout=10,
    )


def _fetch_bin_for_building(building_id: str) -> int | None:
    resp = _meshdb_get(f"/api/v1/buildings/{building_id}/")
    if resp.status_code != 200:
        return None
    return resp.json().get("bin") or None


@app.get("/api/meshdb/resolve-number/<int:number>")
def resolve_meshdb_number(number: int):
    """Resolve a MeshDB NN or install number to a NYC BIN.

    Returns JSON: {"bin": "<7-digit string>", "kind": "nn"|"install"}
    """
    if number <= 0:
        abort(400, f"Invalid number for MeshDB lookup: {number}")

    disambig = _meshdb_get(f"/api/v1/disambiguate-number/?number={number}")
    if disambig.status_code != 200:
        abort(502, f"MeshDB disambiguate failed ({disambig.status_code})")

    data = disambig.json()
    supporting = data.get("supporting_data", {})

    exact_node = supporting.get("exact_match_node")
    exact_install = supporting.get("exact_match_install")

    if exact_node:
        node_resp = _meshdb_get(f"/api/v1/nodes/{exact_node['id']}/")
        if node_resp.status_code != 200:
            abort(502, f"MeshDB node fetch failed ({node_resp.status_code})")
        buildings = node_resp.json().get("buildings") or []
        if not buildings:
            abort(404, "Node has no associated buildings")
        for building_ref in buildings:
            bin_val = _fetch_bin_for_building(building_ref["id"])
            if bin_val:
                return jsonify({"bin": str(bin_val), "kind": "nn"})
        abort(404, "No building with a valid BIN found for this node")

    elif exact_install:
        install_resp = _meshdb_get(f"/api/v1/installs/{exact_install['id']}/")
        if install_resp.status_code != 200:
            abort(502, f"MeshDB install fetch failed ({install_resp.status_code})")
        building_ref = install_resp.json().get("building") or {}
        building_id = building_ref.get("id")
        if not building_id:
            abort(404, "Install has no associated building")
        bin_val = _fetch_bin_for_building(building_id)
        if not bin_val:
            abort(404, "Building has no BIN")
        return jsonify({"bin": str(bin_val), "kind": "install"})

    else:
        abort(404, "Not a recognized NN or install number")
