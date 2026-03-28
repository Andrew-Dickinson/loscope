import re
import uuid

from flask import abort, send_from_directory, send_file, Response

from los_analyzer.backend.app import app, app_cache, obstruction_provider, tile_provider, ortho_provider, TILE_DIR
from los_analyzer.backend.cache.private_cache import Key
from los_analyzer.backend.io.images import serve_pil_image
from los_analyzer.lib.evaluation.rooftop import SamplePointEvaluation
from los_analyzer.lib.fresnel.visualize import create_zone_obj
from los_analyzer.lib.obstructions.visualize import create_obstruction_obj


@app.get("/api/tileview/terrain/tileOverview/<tile_id>")
def get_terrain_tile_overview(tile_id):
    try:
        if not re.match(r"^\d+_\d{2}$", tile_id):
            raise ValueError(f"Invalid tile ID: {tile_id}")
    except (KeyError, TypeError, ValueError) as exc:
        abort(400, str(exc))

    return {
        "obstruction_ids": obstruction_provider.obstruction_ids_for_tile_id(tile_id)
    }

@app.get("/api/tileview/terrain/heightRaster/<tile_id>")
def get_terrain_height_raster(tile_id):
    # TODO: Would it be better to use a CDN style direct browser file access for this?
    try:
        if not re.match(r"^\d+_\d{2}$", tile_id):
            raise ValueError(f"Invalid tile ID: {tile_id}")
    except (KeyError, TypeError, ValueError) as exc:
        abort(400, str(exc))

    tiff_path = tile_provider.get_tile_tiff_path(tile_id)
    if not tiff_path:
        abort(404, f"Terrain data not found for tile ID {tile_id}")

    return send_from_directory(TILE_DIR, tiff_path.name)

@app.get("/api/tileview/terrain/obstructionObj/<obstruction_type>/<obstruction_id>/<tile_id>")
def get_terrain_obstruction_obj(obstruction_type, obstruction_id, tile_id):
    # TODO: Would it be better to use a CDN style direct browser file access for this?
    #  We would need to pre-create the OBJ files, and somehow embed the xy offset for the browser to apply relative
    #  to the terrain mesh
    try:
        if not re.match(r"^[_a-zA-Z]+$", obstruction_type):
            raise ValueError(f"Invalid obstruction type: {obstruction_type}")
        parsed_uuid = str(uuid.UUID(obstruction_id, version=4))
        if not re.match(r"^\d+_\d{2}$", tile_id):
            raise ValueError(f"Invalid tile ID: {tile_id}")
    except (KeyError, TypeError, ValueError) as exc:
        abort(400, str(exc))

    obs = obstruction_provider.get_obstruction(obstruction_type, parsed_uuid)
    if not obs:
        abort(404, f"No obstruction found with ID: {parsed_uuid} and type {obstruction_type}")

    obs_obj = create_obstruction_obj(obs, tile_id)
    if not obs_obj:
        return Response("", 204)

    return send_file(
        obs_obj,
        'model/obj'
    )

@app.get("/api/tileview/terrain/orthoImage/<tile_id>")
def get_terrain_ortho(tile_id):
    # TODO: Would it be better to use a CDN style direct browser file access for this?
    #   Especially since it's really slow
    try:
        if not re.match(r"^\d+_\d{2}$", tile_id):
            raise ValueError(f"Invalid tile ID: {tile_id}")
    except (KeyError, TypeError, ValueError) as exc:
        abort(400, str(exc))

    image = ortho_provider.get_ortho(tile_id)
    if not image:
        return Response(f"Ortho image for tile id {tile_id} not found", 404)

    return serve_pil_image(image, "JPEG", 85)


@app.get("/api/tileView/fresnelSliceObj/<analysis_id>/<tile_id>")
def get_fresnel_slice(analysis_id, tile_id):
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

    zone_obj = create_zone_obj(
        point_evaluation.zone_full,
        tile_id,
    )

    if not zone_obj:
        return Response("", 204)

    return send_file(
        zone_obj,
        'model/obj'
    )

