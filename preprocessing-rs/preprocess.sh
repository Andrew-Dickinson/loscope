#!/bin/bash

set -ex

export PATH="$PATH:target/release/"

echo "#### Converting DEM file into GeoTIFF..."
gdal_translate \
    -of GTiff \
    -ot Int32 \
    -co COMPRESS=LZW \
    -co BIGTIFF=YES \
    ../data/dem-raw/unpacked/DEM_LiDAR_1ft_2010_Improved_NYC.img \
    ../data/dem-raw/whole_city.tif

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

echo "#### Writing obstruction index..."
loscope-preprocessing build-obstruction-index \
    --obstructions "../data/obstructions/" \
    --output "../data/obstructions/_indexes/"

echo "#### De-noising LAS tiles..."
mkdir -p ../data/denoised-las-tiles/
BASENAME='{/}'
find ../data/raw-lidar-tiles/ -maxdepth 1 -name '*.las' | parallel -j16 --joblog /tmp/preprocess.log \
    pdal pipeline ./pdal-config/denoise_pipeline.json \
        "--readers.las.filename=../data/raw-lidar-tiles/$BASENAME" \
        "--writers.las.filename=../data/denoised-las-tiles/$BASENAME"


echo "#### Slicing and rasterizing LAS tiles into subtiles..."
loscope-preprocessing preprocess-tiles --input ../data/denoised-las-tiles \
                                       --osm-land-polys ../data/osm/land-polygons/land_polygons.shp \
                                       --osm-hydro-structures ../data/osm/nyc_hydro_structures.geojson \
                                       --features-db ../data/nyc_dob.db \
                                       --planimetrics-misc-structures ../data/planimetrics/planimetrics-misc-structure-poly.csv \
                                       --planimetrics-hydro-structures ../data/planimetrics/planimetrics-hydro-structure.csv \
                                       --output ../data/preprocessed-lidar-tiles

