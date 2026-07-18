#!/bin/bash

set -ex

echo "#### Converting DEM file into GeoTIFF..."
gdal_translate \
    -of GTiff \
    -ot Int32 \
    -co COMPRESS=LZW \
    -co BIGTIFF=YES \
    ../data/dem-raw/unpacked/DEM_LiDAR_1ft_2010_Improved_NYC.img \
    ../data/dem-raw/whole_city.tif

rm -rf ../data/dem-raw/unpacked/

echo "#### Slicing DEM file into tiles..."
loscope-preprocessing preprocess-dem ../data/dem-raw/whole_city.tif --output ../data/dem-tiles/

echo "#### Building Sqlite DB..."
loscope-preprocessing build-database --output ../data/nyc_dob.db \
      --footprints ../data/input-csvs/building-footprints.csv \
      --tax-lots ../data/input-csvs/tax-lots.csv  \
      --dob-jobs ../data/input-csvs/DOB-Job-Application-Filings.csv   \
      --dob-now-jobs ../data/input-csvs/DOB-NOW-Build-Job-Application-Filings.csv \
      --cos ../data/input-csvs/DOB-NOW-Certificate-of-Occupancy.csv  \
      --permits ../data/input-csvs/DOB-Permit-Issuance.csv  \
      --now-permits ../data/input-csvs/DOB-NOW-Build-Approved-Permits.csv  \
      --condos ../data/input-csvs/Digital-Tax-Map-Condominiums.csv

echo "#### Writing footprint WKT files..."
loscope-preprocessing build-footprint-wkt --db ../data/nyc_dob.db \
  --output ../data/footprint-wkt/

echo "#### Writing obstruction rasters..."
for query_file in ./queries/*.sql; do
  echo "Running $query_file"
  loscope-preprocessing build-obstructions \
    --db ../data/nyc_dob.db \
    --query "$query_file" \
    --output "../data/obstructions/" \
    --dem-cache ../data/dem-tiles
done

loscope-preprocessing import-geo-json ../bundled_geo_data/BridgeObstructions.geojson \
  --type non_surveyed_bridge \
  --output ../data/obstructions \
  --convert-wgs84

echo "#### Writing obstruction index..."
loscope-preprocessing build-obstruction-index \
    --obstructions "../data/obstructions/" \
    --output "../data/obstructions/_indexes/"

ls "../data/obstructions/_indexes/" > "../data/obstructions/_indexes/_manifest.txt"