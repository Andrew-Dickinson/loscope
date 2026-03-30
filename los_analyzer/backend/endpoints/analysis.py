import re
import uuid
from itertools import batched

from flask import request, abort, Response

from los_analyzer.backend.app import app, app_cache, tile_provider, obstruction_provider
from los_analyzer.backend.cache.private_cache import Key
from los_analyzer.backend.io.images import serve_pil_image
from los_analyzer.backend.io.parsing import parse_coords, parse_obstruction_types
from los_analyzer.backend.services.fresnel_kml import build_fresnel_kml
from los_analyzer.backend.services.tile_map import rasterize_intersection_grid_for_tile, fresnel_ellipse_ring, \
    intersection_image_for_tile
from los_analyzer.lib.coordinates.coordinate_translate import translate_from_nys_plane
from los_analyzer.lib.evaluation.rooftop import evaluate_point, SamplePointEvaluation
from los_analyzer.lib.preprocessing.tile_id import tile_id_to_bounds


@app.post("/api/analysis/analyzePointPair")
def point_analysis():
    data = request.get_json(force=True)
    try:
        point_a = parse_coords(data['point_a_nys'])
        point_b = parse_coords(data['point_b_nys'])
        frequency_hz = float(data['frequency_ghz']) * 1_000_000_000
        obstruction_types: str | list[str] = parse_obstruction_types(data.get("obstruction_types", "*"))
    except (KeyError, TypeError, ValueError) as exc:
        abort(400, str(exc))

    analysis_id = str(uuid.uuid4())
    point_evaluation = evaluate_point(point_a, point_b, frequency_hz, tile_provider, obstruction_provider, obstruction_types)

    app_cache.store(Key(SamplePointEvaluation, analysis_id), point_evaluation)

    return {
        "analysis_id": analysis_id,
        "point_a_nys": point_a,
        "point_b_nys": point_b,
        "frequency_hz": frequency_hz,
        "result": point_evaluation.status,
    }

@app.get("/api/analysis/overview/<analysis_id>")
def get_map(analysis_id):
    try:
        parsed_uuid = str(uuid.UUID(analysis_id, version=4))
        cache_key = Key(SamplePointEvaluation, parsed_uuid)
        if not app_cache.contains(cache_key):
            abort(404, f"No such analysis_id: {analysis_id}")
    except (KeyError, TypeError, ValueError) as exc:
        abort(400, str(exc))

    point_evaluation: SamplePointEvaluation = app_cache.fetch(cache_key)

    return {
        "endpoints": [
            translate_from_nys_plane(point_evaluation.point_a_nys),
            translate_from_nys_plane(point_evaluation.point_b_nys)
        ],
        "tiles": [
            {
                "id": tile_id,
                "bounds": [
                    translate_from_nys_plane(coord_pair)
                    for coord_pair in
                    batched(tile_id_to_bounds(tile_id), 2)
                ],
                "intersection_detected": bool(
                    rasterize_intersection_grid_for_tile(
                        tile_id, point_evaluation.intersection_full
                    ).max() > 0
                )
            }
            for tile_id in
            point_evaluation.tile_ids
        ],
        "overhead_ellipse_poly": fresnel_ellipse_ring(
            point_evaluation.point_a_nys,
            point_evaluation.point_b_nys,
            point_evaluation.frequency_hz
        )
    }

@app.get("/api/analysis/intersectionVisualization/<analysis_id>/<tile_id>")
def get_intersection_raster(analysis_id, tile_id):
    try:
        parsed_uuid = str(uuid.UUID(analysis_id, version=4))
        cache_key = Key(SamplePointEvaluation, parsed_uuid)
        if not app_cache.contains(cache_key):
            abort(404, f"No such analysis_id: {analysis_id}")

        if not re.match(r"^\d+_\d{2}$", tile_id):
            raise ValueError(f"Invalid tile ID: {tile_id}")

    except (KeyError, TypeError, ValueError) as exc:
        abort(400, str(exc))

    point_evaluation: SamplePointEvaluation = app_cache.fetch(cache_key)
    image = intersection_image_for_tile(tile_id, point_evaluation.intersection_full)

    if not image:
        return Response(status=204)

    return serve_pil_image(image)

@app.get("/api/analysis/fresnelKml/<analysis_id>")
def get_fresnel_kml(analysis_id):
    try:
        parsed_uuid = str(uuid.UUID(analysis_id, version=4))
        cache_key = Key(SamplePointEvaluation, parsed_uuid)
        if not app_cache.contains(cache_key):
            abort(404, f"No such analysis_id: {analysis_id}")
    except (KeyError, TypeError, ValueError) as exc:
        abort(400, str(exc))

    ev: SamplePointEvaluation = app_cache.fetch(cache_key)
    kml = build_fresnel_kml(
        parsed_uuid,
        translate_from_nys_plane(ev.point_a_nys),
        translate_from_nys_plane(ev.point_b_nys),
        ev.frequency_hz,
    )

    return Response(
        kml,
        mimetype='application/vnd.google-earth.kml+xml',
        headers={'Content-Disposition': f'attachment; filename="{parsed_uuid}.kml"'}
    )
