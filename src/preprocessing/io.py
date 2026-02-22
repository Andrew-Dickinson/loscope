import json
from pathlib import Path

import tifffile


def save_tile(tile, out_dir):
    """Write a TileData to a .tif raster and a .json metadata file."""
    out_dir = Path(out_dir)
    tif_path = out_dir / f"{tile.tile_id}.tif"
    json_path = out_dir / f"{tile.tile_id}.json"

    tifffile.imwrite(str(tif_path), tile.raster)

    metadata = {
        "tile_id": tile.tile_id,
        "x_offset": tile.x_offset,
        "y_offset": tile.y_offset,
        "raster_file": f"{tile.tile_id}.tif",
        "obstruction_ids": tile.obstruction_ids,
    }
    json_path.write_text(json.dumps(metadata, indent=2))


def load_tile(tile_id, in_dir):
    """Load a TileData from a .tif + .json pair in in_dir."""
    from .tile import TileData

    in_dir = Path(in_dir)
    json_path = in_dir / f"{tile_id}.json"
    metadata = json.loads(json_path.read_text())

    tif_path = in_dir / metadata["raster_file"]
    raster = tifffile.imread(str(tif_path))

    return TileData(
        tile_id=metadata["tile_id"],
        x_offset=metadata["x_offset"],
        y_offset=metadata["y_offset"],
        raster=raster,
        obstruction_ids=metadata["obstruction_ids"],
    )
