import pyproj
from pyproj.enums import TransformDirection

# us survey ft^3 in the NY State Survey Plane (Long Island)
nys_crs = pyproj.crs.CRS.from_string("EPSG:6539+6360")

# lat, lon, Meters AMSL (WGS84 + EGM96)
gps_crs = pyproj.crs.CRS.from_string("EPSG:4326+5773")

transformer = pyproj.Transformer.from_crs(nys_crs, gps_crs)

# To NYS survey plane
transformer.transform(40.815807, -73.941304, 26.27198374396749, direction=TransformDirection.INVERSE)

# To WGS84 + EGM96
transformer.transform(1000497.0026113207, 236503.05841742607, 86.194)
