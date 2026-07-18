#!/bin/bash

set -ex

./incremental-preprocess.sh

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
                                       --output ../data/preprocessed-lidar-tiles \
                                       --zero-water-elevation

