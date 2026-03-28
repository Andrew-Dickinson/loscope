from pathlib import Path
from typing import Optional

import PIL
import imagecodecs
import numpy as np
from PIL import Image

from los_analyzer.lib.providers.read_through_cache import ReadThroughCache, AssetProvider

class OrthoProvider:
    def get_ortho(self, tile_id: str) -> Optional[PIL.Image]:
        raise NotImplementedError()

ASSET_TYPE_ORTHO_IMAGE = "ORTHO_IMAGE"

class CachingOrthoProvider(ReadThroughCache, OrthoProvider):
    def __init__(self, upstream: AssetProvider, ortho_dir: Path):
        super().__init__(upstream, ortho_dir)

    def get_ortho(self, tile_id: str) -> Optional[PIL.Image]:
        file_id, subgrid_id = tile_id.rsplit("_", 1)

        if len(subgrid_id) != 2:
            raise ValueError(f"Invalid tile_id: {tile_id}")

        local_ortho_path = self.get_from_fs_cache_or_upstream(ASSET_TYPE_ORTHO_IMAGE, f"{file_id.zfill(6)}.jp2")
        if not local_ortho_path:
            return None

        img_arr = imagecodecs.jpeg2k_decode(local_ortho_path.read_bytes())
        xi = int(subgrid_id[-2])
        yi = int(subgrid_id[-1])
        row0 = (4 - yi) * 1000
        col0 = xi * 1000
        crop = img_arr[row0:row0 + 1000, col0:col0 + 1000, :3]
        return Image.fromarray(crop.astype(np.uint8), mode="RGB")
