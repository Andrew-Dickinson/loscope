import json
from collections import defaultdict
from pathlib import Path
from typing import List, Dict

import numpy as np
import tifffile

from .model import Obstruction


def save_obstruction(obs: Obstruction, out_dir: str | Path) -> None:
    """Write an Obstruction to a .tif raster and a .json metadata file."""
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    tif_path = out_dir / f"{obs.obstruction_id}.tif"
    json_path = out_dir / f"{obs.obstruction_id}.json"

    tifffile.imwrite(str(tif_path), obs.raster)

    width, height = obs.raster.shape
    metadata = {
        "obstruction_id": obs.obstruction_id,
        "obstruction_type": obs.obstruction_type,
        "attributes": obs.attributes,
        "tile_ids": obs.tile_ids,
        "x_offset": obs.x_offset,
        "y_offset": obs.y_offset,
        "width": width,
        "height": height,
        "raster_file": f"{obs.obstruction_id}.tif",
    }
    json_path.write_text(json.dumps(metadata, indent=2))


def load_obstruction(obstruction_id: str, obs_dir: str | Path) -> Obstruction:
    """Load an Obstruction from a .tif + .json pair in obs_dir."""
    obs_dir = Path(obs_dir)
    json_path = obs_dir / f"{obstruction_id}.json"
    meta = json.loads(json_path.read_text())

    tif_path = obs_dir / meta["raster_file"]
    raster = tifffile.imread(str(tif_path))

    return Obstruction(
        obstruction_id=meta["obstruction_id"],
        obstruction_type=meta["obstruction_type"],
        attributes=meta["attributes"],
        tile_ids=meta.get("tile_ids", []),
        x_offset=meta["x_offset"],
        y_offset=meta["y_offset"],
        raster=raster,
    )