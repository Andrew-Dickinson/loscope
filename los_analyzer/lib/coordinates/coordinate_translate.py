import pyproj

nys_crs = pyproj.CRS.from_string("EPSG:6539+6360")
gps_crs = pyproj.CRS.from_string("EPSG:10606+5773")
nys_to_gps = pyproj.Transformer.from_crs(nys_crs, gps_crs, always_xy=True)
gps_to_nys = pyproj.Transformer.from_crs(gps_crs, nys_crs, always_xy=False)

# Hard to know for sure what value to use here, it sorta depends on what the base
# layers in the maps we use are doing, 2020 is a reasonable default as it's the refernce for WGS 84 (G2296), but
# really this could plausibly be anything 2014+
EPOCH = 2020


def translate_to_nys_plane(gps_point: tuple[float, float, float]) -> tuple[float, float, float]:
    return gps_to_nys.transform(*gps_point)

def translate_from_nys_plane(nys_point: tuple[float, float, float]) -> tuple[float, float, float]:
    return nys_to_gps.transform(*nys_point)

