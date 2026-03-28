from typing import Any, Tuple, cast, Literal


def parse_coords(coords: Any) -> Tuple[float, float, float]:
    if not isinstance(coords, list):
        raise TypeError("Coords must be array")

    if len(coords) != 3:
        raise ValueError("Coords must be 3 elements long")

    return cast(Tuple[float, float, float], tuple(float(c) for c in coords))

def parse_obstruction_types(types: Any) -> list[str] | Literal["*"]:
    if not isinstance(types, list):
        if types == "*":
            return types

        raise TypeError("Obstruction types must be array or '*'")

    return list(str(c) for c in types)
