#!/bin/bash

export PATH="$PATH:../target/release/"

set -ex

echo "#### Downloading city data..."
loscope-preprocessing download-city-data --output ../data/input-csvs

echo "#### Downloading monolithic dem file..."
mkdir -p ../data/dem-raw/unpacked
wget --no-verbose https://sa-static-customer-assets-us-east-1-fedramp-prod.s3.amazonaws.com/data.cityofnewyork.us/NYC_DEM_1ft_Float_2.zip \
      -O ../data/dem-raw/NYC_DEM_1ft_Float_2.zip
unzip ../data/dem-raw/NYC_DEM_1ft_Float_2.zip -d ../data/dem-raw/unpacked
rm -rf ../data/dem-raw/NYC_DEM_1ft_Float_2.zip
