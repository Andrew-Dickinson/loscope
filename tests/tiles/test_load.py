"""Tests for los_analyzer.tiles.load"""

from pathlib import Path
from typing import Optional

import numpy as np
import pytest
import tifffile

from los_analyzer.lib.fresnel.fresnel_zone2 import FresnelZone
from los_analyzer.lib.preprocessing.io import save_tile
from los_analyzer.lib.preprocessing.tile import TileData
from los_analyzer.lib.preprocessing.tile_id import tile_id_to_offset
from los_analyzer.lib.providers.obstruction_provider import ObstructionProvider
from los_analyzer.lib.providers.tile_provider import TileProvider
from los_analyzer.lib.tiles.load import TerrainGrid, load_terrain_grid


# ---------------------------------------------------------------------------
# Minimal test-only providers
# ---------------------------------------------------------------------------

class FsTileProvider(TileProvider):
    """Reads .tif files from a local directory; returns an empty tile if missing."""

    def __init__(self, tile_dir: Path):
        self._tile_dir = Path(tile_dir)

    def get_tile(self, tile_id: str) -> Optional[TileData]:
        tif = self._tile_dir / f"{tile_id}.tif"
        x_offset, y_offset = tile_id_to_offset(tile_id)
        if not tif.exists():
            return TileData(
                tile_id=tile_id,
                x_offset=x_offset,
                y_offset=y_offset,
                raster=np.zeros((500, 500), dtype=np.uint16),
            )
        raster = tifffile.imread(str(tif))
        return TileData(tile_id=tile_id, x_offset=x_offset, y_offset=y_offset, raster=raster)


class NoOpObstructionProvider(ObstructionProvider):
    """Returns no obstructions for any tile."""

    def obstruction_ids_for_tile_id(self, tile_id: str):
        return {}

    def get_obstruction(self, obstruction_type: str, obstruction_id: str):
        return None


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _make_zone(x_base, y_base, n_rows, e_offset, e_width):
    """Build a FresnelZone with uniform row geometry."""
    widths = np.full(n_rows, e_width, dtype=np.uint32)
    offsets = np.full(n_rows, e_offset, dtype=np.uint32)
    max_w = max(e_width, 1)
    top = np.zeros((n_rows, max_w), dtype=np.uint16)
    bottom = np.zeros((n_rows, max_w), dtype=np.uint16)
    return FresnelZone(top=top, bottom=bottom, widths=widths, offsets=offsets,
                       x_base_offset=x_base, y_base_offset=y_base)


def _flat_tile(tile_id, x_offset, y_offset, fill=0):
    """Return a TileData with a constant height value across the full 500×500 raster."""
    raster = np.full((500, 500), fill, dtype=np.uint16)
    return TileData(tile_id=tile_id, x_offset=x_offset, y_offset=y_offset,
                    raster=raster)


def _load(zone, tile_ids, tmp_path, obstruction_types='*'):
    return load_terrain_grid(
        zone, tile_ids,
        FsTileProvider(tmp_path),
        obstruction_types,
        NoOpObstructionProvider(),
    )


# ---------------------------------------------------------------------------
# Shape and alignment
# ---------------------------------------------------------------------------

def test_empty_tile_list_returns_zero_heights(tmp_path):
    """When no tiles are provided, TerrainGrid heights should be all zeros."""
    zone = _make_zone(x_base=1001000, y_base=236000, n_rows=10, e_offset=0, e_width=5)
    result = _load(zone, [], tmp_path)
    assert result.heights.shape == (10, 5)
    assert np.all(result.heights == 0)


def test_widths_offsets_match_fresnel_zone(tmp_path):
    """When loaded, TerrainGrid widths and offsets must exactly match the input FresnelZone."""
    zone = _make_zone(x_base=1001000, y_base=236000, n_rows=10, e_offset=50, e_width=100)
    result = _load(zone, [], tmp_path)
    np.testing.assert_array_equal(result.widths, zone.widths)
    np.testing.assert_array_equal(result.offsets, zone.offsets)
    assert result.x_base_offset == zone.x_base_offset
    assert result.y_base_offset == zone.y_base_offset


def test_terrain_grid_base_offsets_copied(tmp_path):
    """When loaded, TerrainGrid x_base_offset and y_base_offset match the FresnelZone."""
    zone = _make_zone(x_base=1005000, y_base=240000, n_rows=5, e_offset=0, e_width=10)
    result = _load(zone, [], tmp_path)
    assert result.x_base_offset == 1005000
    assert result.y_base_offset == 240000


# ---------------------------------------------------------------------------
# Tile blitting
# ---------------------------------------------------------------------------

def test_tile_heights_appear_at_correct_grid_positions(tmp_path):
    """When a tile is loaded, its raster values land at the correct (row, col) positions."""
    # Zone: 10 rows starting at northing 236000; eastings [1001000, 1001100)
    zone = _make_zone(x_base=1001000, y_base=236000, n_rows=10, e_offset=0, e_width=100)

    # Build a raster with unique values per cell: raster[dx, dy] = dx * 100 + dy + 1
    raster = np.zeros((500, 500), dtype=np.uint16)
    raster[:100, :10] = (np.arange(100).reshape(100, 1) * 100 +
                         np.arange(10).reshape(1, 10) + 1)
    tile = TileData("235_22", x_offset=1001000, y_offset=236000, raster=raster)
    save_tile(tile, tmp_path)

    result = _load(zone, ["235_22"], tmp_path)

    # Row i=3 (northing 236003), col j=5 (easting 1001005) → dy=3, dx=5 → 5*100+3+1=504
    assert result.heights[3, 5] == 504


def test_full_tile_overlap_fills_all_cells(tmp_path):
    """When a tile fully covers the zone, every zone cell gets the tile's height."""
    zone = _make_zone(x_base=1001000, y_base=236000, n_rows=10, e_offset=0, e_width=50)
    save_tile(_flat_tile("235_22", x_offset=1001000, y_offset=236000, fill=1234), tmp_path)
    result = _load(zone, ["235_22"], tmp_path)
    assert np.all(result.heights[:, :50] == 1234)


def test_tile_partially_overlapping_zone_easting(tmp_path):
    """When a tile covers only the eastern half of the zone, only that half is filled."""
    # Zone: eastings [1000800, 1001200) (400 wide)
    zone = _make_zone(x_base=1000800, y_base=236000, n_rows=5, e_offset=0, e_width=400)
    # Tile starts at easting 1001000 — covers only cols 200..399 of the zone
    save_tile(_flat_tile("235_22", x_offset=1001000, y_offset=236000, fill=999), tmp_path)
    result = _load(zone, ["235_22"], tmp_path)
    assert np.all(result.heights[:, :200] == 0)
    assert np.all(result.heights[:, 200:400] == 999)


def test_tile_outside_zone_northing_leaves_heights_zero(tmp_path):
    """When a tile's northing range does not overlap the zone rows, heights remain zero."""
    # Zone: northings [236000, 236010)
    zone = _make_zone(x_base=1001000, y_base=236000, n_rows=10, e_offset=0, e_width=50)
    # Tile starts at northing 236500 — entirely above the zone
    save_tile(_flat_tile("235_23", x_offset=1001000, y_offset=236500, fill=777), tmp_path)
    result = _load(zone, ["235_23"], tmp_path)
    assert np.all(result.heights == 0)


def test_two_non_overlapping_tiles_each_fill_their_area(tmp_path):
    """When two adjacent tiles are loaded, each fills its own easting range."""
    # Zone: eastings [1001000, 1002000) — spans two 500-usft tiles
    zone = _make_zone(x_base=1001000, y_base=236000, n_rows=5, e_offset=0, e_width=1000)
    save_tile(_flat_tile("235_22", x_offset=1001000, y_offset=236000, fill=100), tmp_path)
    save_tile(_flat_tile("235_32", x_offset=1001500, y_offset=236000, fill=200), tmp_path)
    result = _load(zone, ["235_22", "235_32"], tmp_path)
    assert np.all(result.heights[:, :500] == 100)
    assert np.all(result.heights[:, 500:1000] == 200)


def test_tile_at_northing_boundary_fills_correct_rows(tmp_path):
    """When a zone spans a tile northing boundary, each tile fills only its rows."""
    # Zone: 1000 rows starting at northing 236000; eastings [1001000, 1001050)
    zone = _make_zone(x_base=1001000, y_base=236000, n_rows=1000, e_offset=0, e_width=50)
    # tile_22: northing [236000, 236500) → rows 0..499 of zone
    save_tile(_flat_tile("235_22", x_offset=1001000, y_offset=236000, fill=11), tmp_path)
    # tile_23: northing [236500, 237000) → rows 500..999 of zone
    save_tile(_flat_tile("235_23", x_offset=1001000, y_offset=236500, fill=22), tmp_path)
    result = _load(zone, ["235_22", "235_23"], tmp_path)
    assert np.all(result.heights[:500, :50] == 11)
    assert np.all(result.heights[500:, :50] == 22)


# ---------------------------------------------------------------------------
# Obstruction handling
# ---------------------------------------------------------------------------

def test_matched_obstruction_ids_empty_when_no_obstructions(tmp_path):
    """When tiles have no additional obstructions, matched_obstruction_ids is empty."""
    zone = _make_zone(x_base=1001000, y_base=236000, n_rows=5, e_offset=0, e_width=10)
    save_tile(_flat_tile("235_22", x_offset=1001000, y_offset=236000), tmp_path)
    result = _load(zone, ["235_22"], tmp_path)
    assert not result.matched_obstruction_ids


def test_matched_obstruction_ids_empty_when_no_tiles(tmp_path):
    """When no tiles are loaded, matched_obstruction_ids is empty regardless of filter."""
    zone = _make_zone(x_base=1001000, y_base=236000, n_rows=5, e_offset=0, e_width=10)
    result = _load(zone, [], tmp_path, ["Existing Building Footprint"])
    assert not result.matched_obstruction_ids
