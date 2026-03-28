import sqlite3
from pathlib import Path

from arcgis.geometry import BaseGeometry
from shapely import wkt

# TODO: Standardize "not found" handling
# TODO: Upstream fetch

class DOBDBDAO:
    def __init__(self, db_path: Path):
        self.db_path = db_path

    def fetch_building_footprint_geometry(self, bin_id: str) -> BaseGeometry:
        """Return the NYS EPSG:6539 shapely geometry for the given BIN.

        Queries the building_footprints table in *db_path* for the row whose
        ``bin`` column equals *bin_id*.

        Raises:
            ValueError: If no matching row is found, the geometry field is empty,
                or the WKT cannot be parsed.
        """
        con = sqlite3.connect(self.db_path)
        con.execute("PRAGMA query_only=ON")
        row = con.execute(
            "SELECT the_geom FROM building_footprints WHERE bin = ? LIMIT 1",
            (bin_id,),
        ).fetchone()
        con.close()

        if row is None or not row[0]:
            raise ValueError(f"BIN {bin_id!r} not found in building_footprints")

        try:
            geom = wkt.loads(row[0])
        except Exception as exc:
            raise ValueError(f"Could not parse geometry for BIN {bin_id!r}: {exc}") from exc

        if geom.is_empty:
            raise ValueError(f"Empty geometry for BIN {bin_id!r}")

        return geom