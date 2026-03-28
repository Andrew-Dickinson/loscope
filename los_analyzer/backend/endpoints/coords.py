from flask import request, abort
from los_analyzer.backend.app import app
from los_analyzer.lib.coordinates.coordinate_translate import translate_to_nys_plane


@app.post("/api/coords/toNys")
def gps_to_nys():
    data = request.get_json(force=True)
    try:
        lat = float(data['lat'])
        lon = float(data['lon'])
        alt_m = float(data['alt_m'])
    except (KeyError, TypeError, ValueError) as exc:
        abort(400, str(exc))

    e, n, z = translate_to_nys_plane((lat, lon, alt_m))
    return {"nys_e": e, "nys_n": n, "nys_z": z}
