TILE_SIDE_USFT = 500
LAS_SIDE_USFT = 2500
GRID_N = 5


def fname_int_to_coordinate(fname_str):
    if len(fname_str) > 0:
        fname_int = int(fname_str)
    else:
        fname_int = 0

    if fname_int % 5 != 0:
        return fname_int * 1000 + 500
    return fname_int * 1000


def file_id_to_offset(file_id):
    x_min = fname_int_to_coordinate(file_id[:-3])
    y_min = fname_int_to_coordinate(file_id[-3:])
    if x_min < 500000:
        x_min += 1000000

    return (x_min, y_min)


def make_tile_id(file_id, xi, yi):
    return f"{file_id}_{xi}{yi}"


def tile_sw_corner(origin, xi, yi):
    """Return the SW corner (min X, min Y) of tile at grid position (xi, yi)."""
    return (origin[0] + xi * TILE_SIDE_USFT, origin[1] + yi * TILE_SIDE_USFT)
