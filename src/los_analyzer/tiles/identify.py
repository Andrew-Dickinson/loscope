from __future__ import annotations

from pathlib import Path

from los_analyzer.fresnel.fresnel_zone2 import FresnelZone
from los_analyzer.preprocessing.tile_id import LAS_SIDE_USFT, TILE_SIDE_USFT, file_id_to_offset, make_tile_id

# SW corner of the canonical LAS tile grid (file_id "912117").
_GRID_BASE_E = 912500
_GRID_BASE_N = 117500


def identify_tiles(
    fresnel_zone: FresnelZone,
    tile_dir: str | Path | None = None,
    *,
    require_exists: bool = True,
) -> list[str]:
    """Return IDs of preprocessed tiles that overlap the fresnel zone.

    Derives tile membership from the FresnelZone width-offset encoding: for each
    row i, the occupied eastings run from x_base_offset + offsets[i] to
    x_base_offset + offsets[i] + widths[i] - 1.

    Args:
        fresnel_zone: The FresnelZone from Step 2.1.
        tile_dir: Directory containing preprocessed tile .json files. Required
            when require_exists=True; ignored when require_exists=False.
        require_exists: If True (default), scan tile_dir for *.json files and
            return only those that overlap the zone. If False, compute tile IDs
            purely from coordinates using the canonical grid, with no filesystem
            access needed.
    """
    # Collect the set of 500-usft SW-corner (easting, northing) positions covered.
    covered: set[tuple[int, int]] = set()
    for i in range(len(fresnel_zone.widths)):
        width = int(fresnel_zone.widths[i])
        if width == 0:
            continue
        northing = fresnel_zone.y_base_offset + i
        n_base = (northing // TILE_SIDE_USFT) * TILE_SIDE_USFT

        e_min = fresnel_zone.x_base_offset + int(fresnel_zone.offsets[i])
        e_max = e_min + width - 1
        e_base = (e_min // TILE_SIDE_USFT) * TILE_SIDE_USFT
        e_base_max = (e_max // TILE_SIDE_USFT) * TILE_SIDE_USFT
        while e_base <= e_base_max:
            covered.add((e_base, n_base))
            e_base += TILE_SIDE_USFT

    if not require_exists:
        result = []
        for pos in sorted(covered):
            tile_id = _tile_id_from_sw_corner(*pos)
            if tile_id is not None:
                result.append(tile_id)
        return result

    if tile_dir is None:
        raise ValueError("tile_dir is required when require_exists=True")
    result = []
    for json_path in sorted(Path(tile_dir).glob("*.json")):
        tile_id = json_path.stem
        file_id, xi, yi = _parse_tile_id(tile_id)
        if file_id is None:
            continue
        origin = file_id_to_offset(file_id)
        e_base = origin[0] + xi * TILE_SIDE_USFT
        n_base = origin[1] + yi * TILE_SIDE_USFT
        if (e_base, n_base) in covered:
            result.append(tile_id)

    return result


def _tile_id_from_sw_corner(e_base: int, n_base: int) -> str | None:
    """Return the canonical tile_id for the subtile whose SW corner is (e_base, n_base).

    Uses the canonical LAS grid (base SW corner 912500, 117500; spacing 2500 usft)
    to uniquely snap any 500-usft-aligned position to its containing LAS tile and
    subtile indices.
    """
    las_e = e_base - (e_base - _GRID_BASE_E) % LAS_SIDE_USFT
    las_n = n_base - (n_base - _GRID_BASE_N) % LAS_SIDE_USFT
    xi = (e_base - las_e) // TILE_SIDE_USFT
    yi = (n_base - las_n) // TILE_SIDE_USFT
    file_id = _coord_to_file_id(las_e, las_n)
    if file_id is None:
        return None
    return make_tile_id(file_id, xi, yi)


def _coord_to_file_id(e: int, n: int) -> str | None:
    """Return the file_id whose file_id_to_offset equals (e, n), or None if invalid."""
    x_raw = e - 1000000 if 1000000 <= e < 1500000 else e
    x_int = _fname_int_from_coord(x_raw)
    y_int = _fname_int_from_coord(n)
    if x_int is None or y_int is None:
        return None
    x_part = "" if x_int == 0 else str(x_int)
    y_part = str(y_int).zfill(3)
    return x_part + y_part


def _fname_int_from_coord(coord: int) -> int | None:
    """Reverse fname_int_to_coordinate: return the integer that maps to coord, or None."""
    if coord % 1000 == 0:
        n = coord // 1000
        return n if n % 5 == 0 else None
    if coord % 1000 == 500:
        n = coord // 1000
        return n if n % 5 != 0 else None
    return None


def _parse_tile_id(tile_id: str) -> tuple[str | None, int, int]:
    """Parse '{file_id}_{xi}{yi}' into (file_id, xi, yi), or (None, 0, 0) on error."""
    parts = tile_id.rsplit("_", 1)
    if len(parts) != 2 or len(parts[1]) != 2:
        return None, 0, 0
    try:
        xi = int(parts[1][0])
        yi = int(parts[1][1])
    except ValueError:
        return None, 0, 0
    return parts[0], xi, yi
