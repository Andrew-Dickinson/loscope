from flask import abort, send_file, request

from los_analyzer.backend.app import app, dob_db_dao, tile_provider
from los_analyzer.backend.services.rooftop import build_building_heightmap_cached, build_rooftop_obj, \
    get_paired_sample_points_cached


@app.get("/api/rooftop/render/<bin_id>")
def render_rooftop(bin_id):
    try:
        bin_id_parsed = str(int(bin_id))
    except (KeyError, TypeError, ValueError) as exc:
        abort(400, str(exc))

    # TODO: 404 for bad bin
    heightmap = build_building_heightmap_cached(bin_id_parsed, dob_db_dao, tile_provider)

    return send_file(
        build_rooftop_obj(bin_id_parsed, heightmap.heightmap),
        'model/obj'
    )

@app.get("/api/rooftop/samplePoints/<bin_id>")
def sample_rooftop_points(bin_id):
    data = request.get_json(force=True)
    try:
        bin_id_parsed = str(int(bin_id))
        mast_offset = float(data.get("mast_offset_ft"))
        spacing = int(data.get("sample_spacing"))
    except (KeyError, TypeError, ValueError) as exc:
        abort(400, str(exc))

    # TODO: 404 for bad bin
    heightmap = build_building_heightmap_cached(bin_id_parsed, dob_db_dao, tile_provider)
    return {
        "sample_points": get_paired_sample_points_cached(heightmap, spacing, mast_offset)
    }

