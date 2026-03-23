"""Flask application entry point."""
from __future__ import annotations

import os
from pathlib import Path

from flask import Flask, abort, jsonify, request, send_file
from flask_cors import CORS

from tasks.manager import TaskManager
from utils.cache import cache_key, load_cache, save_cache

app = Flask(__name__)
CORS(app)

# ── Config from environment ──────────────────────────────────────────────────
_ROOT = Path(__file__).resolve().parent.parent   # repo root
DB_PATH    = Path(os.environ.get("LOS_DB_PATH",    _ROOT / "data/nyc_dob.db"))
TILE_DIR   = Path(os.environ.get("LOS_TILE_DIR",   _ROOT / "data/preprocessed"))
OBS_DIR    = Path(os.environ.get("LOS_OBS_DIR",    _ROOT / "data/obstructions"))
ORTHO_DIR  = Path(os.environ.get("LOS_ORTHO_DIR",  _ROOT / "data/orthos"))
CACHE_DIR  = Path(os.environ.get("LOS_CACHE_DIR",  _ROOT / "data/web_cache"))

tm = TaskManager()


# ── Job polling ──────────────────────────────────────────────────────────────

@app.get("/api/jobs/<job_id>")
def get_job(job_id: str):
    job = tm.get(job_id)
    if job is None:
        abort(404)
    return jsonify(tm.to_dict(job))


# ── Rooftop evaluation ───────────────────────────────────────────────────────

@app.post("/api/evaluate-rooftop")
def evaluate_rooftop():
    data = request.get_json(force=True)
    try:
        bin_id       = str(data["bin_id"])
        lat          = float(data["lat"])
        lon          = float(data["lon"])
        alt_m        = float(data["alt_m"])
        freq_ghz     = float(data.get("frequency_ghz", 24.0))
        mast_offset  = float(data.get("mast_offset_ft", 0.0))
        spacing      = int(data.get("sample_spacing", 5))
    except (KeyError, TypeError, ValueError) as exc:
        abort(400, str(exc))

    inputs = {
        "bin_id": bin_id, "lat": lat, "lon": lon, "alt_m": alt_m,
        "frequency_ghz": freq_ghz, "mast_offset_ft": mast_offset,
        "sample_spacing": spacing,
    }
    key = cache_key(inputs)
    cached = load_cache("rooftop", key)
    if cached:
        # Return a synthetic already-done job
        job_id = cached["job_id"]
        existing = tm.get(job_id)
        if existing and existing.status == "done":
            return jsonify({"job_id": job_id})
        # Job not in memory (server restart) — rebuild it
        from tasks.manager import Job
        job = Job(job_id=job_id, status="done", progress_pct=100, result=cached)
        with tm._lock:
            tm._jobs[job_id] = job
        return jsonify({"job_id": job_id})

    from services.rooftop import run_rooftop_service
    from uuid import uuid4
    frequency_hz = freq_ghz * 1e9
    job_id = uuid4().hex

    def task(progress_cb):
        result = run_rooftop_service(
            bin_id=bin_id,
            gps_b=(lat, lon, alt_m),
            frequency_hz=frequency_hz,
            progress_cb=progress_cb,
            db_path=DB_PATH,
            tile_dir=TILE_DIR,
            obstruction_dir=OBS_DIR,
            sample_spacing=spacing,
            mast_offset=mast_offset,
            job_id=job_id,
            cache_dir=CACHE_DIR,
        )
        save_cache("rooftop", key, result)
        return result

    tm.submit(task, job_id=job_id)
    return jsonify({"job_id": job_id})


@app.get("/api/rooftop/<job_id>/terrain.obj")
def get_rooftop_terrain(job_id: str):
    job = tm.get(job_id)
    if job is None or job.status != "done" or not job.result:
        abort(404)
    obj_hash = job.result.get("_obj_hash", job_id)
    obj_path = CACHE_DIR / "rooftop" / obj_hash / "terrain.obj"
    if not obj_path.exists():
        abort(404)
    return send_file(obj_path, mimetype="text/plain")


# ── Tile map ─────────────────────────────────────────────────────────────────

@app.post("/api/tile-map")
def tile_map():
    data = request.get_json(force=True)
    try:
        nys_a      = tuple(float(v) for v in data["nys_a"])
        nys_b      = tuple(float(v) for v in data["nys_b"])
        freq_ghz   = float(data.get("frequency_ghz", 24.0))
    except (KeyError, TypeError, ValueError) as exc:
        abort(400, str(exc))

    inputs = {"nys_a": list(nys_a), "nys_b": list(nys_b), "frequency_ghz": freq_ghz}
    key = cache_key(inputs)
    cached = load_cache("tile_map", key)
    if cached:
        job_id = cached["job_id"]
        existing = tm.get(job_id)
        if existing and existing.status == "done":
            return jsonify({"job_id": job_id})
        from tasks.manager import Job
        job = Job(job_id=job_id, status="done", progress_pct=100, result=cached)
        with tm._lock:
            tm._jobs[job_id] = job
        return jsonify({"job_id": job_id})

    from services.tile_map import run_tile_map_service
    frequency_hz = freq_ghz * 1e9

    def task(progress_cb):
        result = run_tile_map_service(
            nys_a=nys_a,
            nys_b=nys_b,
            frequency_hz=frequency_hz,
            progress_cb=progress_cb,
            tile_dir=TILE_DIR,
            obstruction_dir=OBS_DIR,
            ortho_dir=ORTHO_DIR,
        )
        save_cache("tile_map", key, result)
        return result

    job_id = tm.submit(task)
    return jsonify({"job_id": job_id})


# ── Tile 3D ──────────────────────────────────────────────────────────────────

@app.post("/api/tile-3d")
def tile_3d():
    data = request.get_json(force=True)
    try:
        tile_id  = str(data["tile_id"])
        nys_a    = tuple(float(v) for v in data["nys_a"])
        nys_b    = tuple(float(v) for v in data["nys_b"])
        freq_ghz = float(data.get("frequency_ghz", 24.0))
    except (KeyError, TypeError, ValueError) as exc:
        abort(400, str(exc))

    inputs = {
        "tile_id": tile_id, "nys_a": list(nys_a),
        "nys_b": list(nys_b), "frequency_ghz": freq_ghz,
    }
    key = cache_key(inputs)
    cached = load_cache("tile_3d", key)
    if cached:
        job_id = cached["job_id"]
        existing = tm.get(job_id)
        if existing and existing.status == "done":
            return jsonify({"job_id": job_id})
        from tasks.manager import Job
        job = Job(job_id=job_id, status="done", progress_pct=100, result=cached)
        with tm._lock:
            tm._jobs[job_id] = job
        return jsonify({"job_id": job_id})

    from services.tile_3d import run_tile_3d_service
    frequency_hz = freq_ghz * 1e9

    def task(progress_cb):
        result = run_tile_3d_service(
            tile_id=tile_id,
            nys_a=nys_a,
            nys_b=nys_b,
            frequency_hz=frequency_hz,
            progress_cb=progress_cb,
            tile_dir=TILE_DIR,
            obstruction_dir=OBS_DIR,
            ortho_dir=ORTHO_DIR,
            cache_dir=CACHE_DIR,
        )
        save_cache("tile_3d", key, result)
        return result

    job_id = tm.submit(task)
    return jsonify({"job_id": job_id})


def _tile_3d_asset_dir(job_id: str) -> Path | None:
    """Return the on-disk directory for a tile-3d job's OBJ assets."""
    job = tm.get(job_id)
    if job is None or job.status != "done" or not job.result:
        return None
    # Service stores files under cache_dir/tile_3d/<inner_job_id>/
    inner_id = job.result.get("job_id", job_id)
    return CACHE_DIR / "tile_3d" / inner_id


@app.get("/api/tile-3d/<job_id>/zone.obj")
def get_tile_zone_obj(job_id: str):
    asset_dir = _tile_3d_asset_dir(job_id)
    if asset_dir is None:
        abort(404)
    obj_path = asset_dir / "zone.obj"
    if not obj_path.exists():
        abort(404)
    return send_file(obj_path, mimetype="text/plain")


@app.get("/api/tile-3d/<job_id>/<obs_id>.obj")
def get_tile_obs_obj(job_id: str, obs_id: str):
    job = tm.get(job_id)
    if job is None or job.status != "done":
        abort(404)
    # Security: only serve OBJ files listed in the job result
    allowed = job.result.get("obstruction_ids", []) if job.result else []
    if obs_id not in allowed:
        abort(403)
    asset_dir = _tile_3d_asset_dir(job_id)
    if asset_dir is None:
        abort(404)
    obj_path = asset_dir / f"{obs_id}.obj"
    if not obj_path.exists():
        abort(404)
    return send_file(obj_path, mimetype="text/plain")


if __name__ == "__main__":
    app.run(debug=True, use_reloader=False, threaded=True, port=5000)
