#!/bin/bash

# TODO: Test me on an EC2 instance

set +ex

export PATH="$PATH:target/release/"

echo "#### Slicing DEM file into tiles..."
loscope-preprocessing preprocess-dem ../data/dem-raw/unpacked/something.tiff \
        --output ../data/dem-tiles/

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

# TODO: Parameterize dates in these query files
echo "#### Writing obstruction rasters..."
for query_file in ./queries/*.sql; do
  query=$(basename "$query_file" .sql)
  loscope-preprocessing build-obstructions \
    --db ../data/nyc_dob.db \
    --query "$query_file" \
    --output "../data/obstructions/${query}/" \
    --dem-cache ../data/dem-tiles
done

echo "#### Writing obstruction index..."
loscope-preprocessing build-obstruction-index \
    --obstructions "../data/obstructions/" \
    --output "../data/obstructions/_indexes/"

echo "#### De-noising LAS tiles..."
ls ../data/raw-lidar-tiles/*.las | cut -d. -f1 | cut -d"/" -f4 | parallel -j4 --progress --joblog tmp/preprocess.log \
    docker run \
        -v ./pdal-config/denoise_pipeline.json:/denoise_pipeline.json \
        -v ../data/:/data \
        pdal/pdal \
            pdal pipeline /denoise_pipeline.json \
                --readers.las.filename=/data/raw-lidar-tiles/{}.las \
                --writers.las.filename=/data/denoised-las-tiles/{}.las


echo "#### Slicing and rasterizing LAS tiles into subtiles..."
loscope-preprocessing preprocess-tiles --input ../data/denoised-las-tiles \
                                       --osm-land-polys ../data/osm/land-polygons/land_polygons.shp \
                                       --osm-hydro-structures ../data/osm/nyc_hydro_structures.geojson \
                                       --features-db ../data/nyc_dob.db \
                                       --planimetrics-misc-structures ../data/planimetrics/planimetrics-misc-structure-poly.csv \
                                       --planimetrics-hydro-structures ../data/planimetrics/planimetrics-hydro-structure.csv \
                                       --output ../data/preprocessed-lidar-tiles

