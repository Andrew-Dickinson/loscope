#!/bin/bash

# TODO: Test me on an EC2 instance

set +ex

export PATH="$PATH:target/release/"

echo "#### Downloading Lidar Data..."
ncftpls ftp://ftp.gis.ny.gov/elevation/LIDAR/NYC_2021/ > /tmp/nys-ftp/tiles.txt
sed -i -e 's/^/ftp:\/\/ftp.gis.ny.gov\/elevation\/LIDAR\/NYC_2021\//' /tmp/nys-ftp/tiles.txt
mkdir ../data/raw-lidar-tiles
cd ../data/raw-lidar-tiles && aria2c -x 1 -j 3 -i /tmp/nys-ftp/tiles.txt

echo "#### Downloading NYC orthoimagery..."
wget -r ftp://ftp.gis.ny.gov//ortho/nysdop10/new_york_city/spcs/zips

echo "#### Downloading city data..."
loscope-preprocessing download-city-data --output ../data/input-csvs

echo "#### Downloading planimetrics data..."
loscope-preprocessing download-planimetrics --output ../data/planimetrics \
 --layer misc-structure-poly
 loscope-preprocessing download-planimetrics --output ../data/planimetrics \
 --layer hydro-structure

echo "#### Downloading monolithic dem file..."
wget https://sa-static-customer-assets-us-east-1-fedramp-prod.s3.amazonaws.com/data.cityofnewyork.us/NYC_DEM_1ft_Float_2.zip \
      -O ../data/dem-raw/NYC_DEM_1ft_Float_2.zip
unzip ../data/dem-raw/NYC_DEM_1ft_Float_2.zip -d ../data/dem-raw/unpacked

echo "#### Downloading OSM land boundaries polygon..."
wget https://osmdata.openstreetmap.de/download/land-polygons-split-4326.zip \
     -O ../data/osm/land-polygons-split-4326.zip
unzip ../data/osm/land-polygons-split-4326.zip -d ../data/osm/land-polygons/


echo "#### Downloading NYC OSM hydro structure features..."
curl -s -G https://overpass-api.de/api/interpreter \
  --data-urlencode "data=[out:json][timeout:90];
(
  nwr[\"man_made\"=\"breakwater\"]($BBOX);
  nwr[\"man_made\"=\"pier\"]($BBOX);
  nwr[\"man_made\"=\"bridge\"]($BBOX);
  nwr[\"bridge\"]($BBOX);
);
out body;
>;
out skel qt;" \
| osmtogeojson > ../data/osm/nyc_hydro_structures.geojson
