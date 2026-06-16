#!/bin/bash

# TODO: Test e2e on EC2

set +ex

export PATH="$PATH:target/release/"

mkdir -p ../data/

echo "#### Downloading Lidar Data..."
mkdir -p /tmp/nys-ftp/
ncftpls ftp://ftp.gis.ny.gov/elevation/LIDAR/NYC_2021/ > /tmp/nys-ftp/tiles.txt
sed -i -e 's/^/ftp:\/\/ftp.gis.ny.gov\/elevation\/LIDAR\/NYC_2021\//' /tmp/nys-ftp/tiles.txt
mkdir -p ../data/raw-lidar-tiles
cd ../data/raw-lidar-tiles && aria2c -x 1 -j 10 -i /tmp/nys-ftp/tiles.txt

echo "#### Downloading NYC orthoimagery..."
mkdir -p ../data/orthos/
cd ../data/orthos/ && wget -r ftp://ftp.gis.ny.gov/ortho/nysdop10/new_york_city/spcs/zips
mv ../data/orthos/ftp.gis.ny.gov/ortho/nysdop10/new_york_city/spcs/zips ../data/orthos/zipped/
rm -rf ../data/orthos/ftp.gis.ny.gov/
for zip_file in ../data/orthos/zipped/*.zip; do
  unzip -o $zip_file -d ../data/orthos/flat/
done

echo "#### Downloading city data..."
loscope-preprocessing download-city-data --output ../data/input-csvs

echo "#### Downloading planimetrics data..."
loscope-preprocessing download-planimetrics --output ../data/planimetrics \
 --layer misc-structure-poly
loscope-preprocessing download-planimetrics --output ../data/planimetrics \
 --layer hydro-structure

echo "#### Downloading monolithic dem file..."
mkdir -p ../data/dem-raw/unpacked
wget https://sa-static-customer-assets-us-east-1-fedramp-prod.s3.amazonaws.com/data.cityofnewyork.us/NYC_DEM_1ft_Float_2.zip \
      -O ../data/dem-raw/NYC_DEM_1ft_Float_2.zip
unzip ../data/dem-raw/NYC_DEM_1ft_Float_2.zip -d ../data/dem-raw/unpacked

echo "#### Downloading OSM land boundaries polygon..."
mkdir -p ../data/osm/land-polygons/
wget https://osmdata.openstreetmap.de/download/land-polygons-split-4326.zip \
     -O ../data/osm/land-polygons-split-4326.zip
unzip ../data/osm/land-polygons-split-4326.zip -d ../data/osm/land-polygons/
mv ../data/osm/land-polygons/land-polygons-split-4326/* ../data/osm/land-polygons/
rmdir ../data/osm/land-polygons/land-polygons-split-4326/


echo "#### Downloading NYC OSM hydro structure features..."
curl -s -G https://overpass-api.de/api/interpreter \
  --data-urlencode "data=[out:json][timeout:90];
(
 nwr[\"man_made\"=\"breakwater\"](40.47204119039251,-74.25933837890626,40.92285137359093,-73.69903564453126);
 nwr[\"man_made\"=\"pier\"](40.47204119039251,-74.25933837890626,40.92285137359093,-73.69903564453126);
 nwr[\"man_made\"=\"bridge\"](40.47204119039251,-74.25933837890626,40.92285137359093,-73.69903564453126);
 nwr[\"bridge\"](40.47204119039251,-74.25933837890626,40.92285137359093,-73.69903564453126);
);
out body;
>;
out skel qt;" \
| osmtogeojson > ../data/osm/nyc_hydro_structures.geojson
