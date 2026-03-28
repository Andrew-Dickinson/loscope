import json
from collections import defaultdict
from pathlib import Path
from typing import Dict, List, Optional

from tifffile import tifffile

from los_analyzer.lib.obstructions.model import Obstruction
from los_analyzer.lib.providers.read_through_cache import ReadThroughCache, AssetProvider

class ObstructionProvider:
    def get_obstruction(self, obstruction_type: str, obstruction_id: str) -> Optional[Obstruction]:
        raise NotImplementedError()

    def obstruction_ids_for_tile_id(self, tile_id: str) -> Dict[str, List[str]]:
        raise NotImplementedError()

ASSET_TYPE_OBSTRUCTION_RASTER =  "OBSTRUCTION_RASTER"
ASSET_TYPE_OBSTRUCTION_DETAIL =  "OBSTRUCTION_DETAIL"
ASSET_TYPE_OBSTRUCTION_INDEXES =  "OBSTRUCTION_INDEXES"

class CachingObstructionProvider(ReadThroughCache, ObstructionProvider):
    def __init__(self, upstream: AssetProvider, obs_dir: Path):
        super().__init__(upstream, obs_dir)

    def get_obstruction(self, obstruction_type: str, obstruction_id: str) -> Optional[Obstruction]:
        detail_path = self.get_from_fs_cache_or_upstream(
            ASSET_TYPE_OBSTRUCTION_DETAIL,
            f"{obstruction_type}/{obstruction_id}.json"
        )
        raster_path = self.get_from_fs_cache_or_upstream(
            ASSET_TYPE_OBSTRUCTION_RASTER,
            f"{obstruction_type}/{obstruction_id}.tif"
        )

        if not detail_path or not raster_path:
            return None

        meta = json.loads(detail_path.read_text())
        raster = tifffile.imread(raster_path)

        return Obstruction(
            obstruction_id=meta["obstruction_id"],
            obstruction_type=meta["obstruction_type"],
            attributes=meta["attributes"],
            tile_ids=meta.get("tile_ids", []),
            x_offset=meta["x_offset"],
            y_offset=meta["y_offset"],
            raster=raster,
        )

    def obstruction_ids_for_tile_id(self, tile_id: str) -> Dict[str, List[str]]:
        index_base_path = self.get_folder_from_fs_cache_or_upstream(ASSET_TYPE_OBSTRUCTION_INDEXES, "_indexes")

        if not index_base_path:
            raise FileNotFoundError("We need to have access to obstruction indexes to find relevant obstruction IDs")

        obstructions_by_type = defaultdict(list)
        for index_path in sorted(index_base_path.glob("*.json")):
            obs_type = index_path.name.removesuffix(".json")
            index = json.loads(index_path.read_text())
            tile_obstructions = index.get(tile_id)
            if tile_obstructions:
                obstructions_by_type[obs_type].extend(tile_obstructions)

        return dict(obstructions_by_type)
