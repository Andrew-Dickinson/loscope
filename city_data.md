#### Building Footprints
Outlines: https://data.cityofnewyork.us/City-Government/BUILDING/5zhs-2jue/about_data
Centroids: https://data.cityofnewyork.us/City-Government/BUILDING_P/u9wf-3gbt/about_data
- Actions
- Query Data
- Filter: `Construction Year` `is greater than or equal to` `2021`
- Export -> Download File -> CSV -> Download

```
python tools/build_building_obstructions.py data/new-building-footprints/BUILDING_20260305.csv data/obstructions/building-footprints/
```

Interactive:
https://data.cityofnewyork.us/City-Government/Building-Footprints-Map-/jh45-qr5r

https://github.com/CityOfNewYork/nyc-geo-metadata/blob/main/Metadata/Metadata_BuildingFootprints.md

#### DEM
Older, but the ground really doesn't move that fast:
https://data.cityofnewyork.us/City-Government/1-foot-Digital-Elevation-Model-DEM-Integer-Raster/7kuu-zah7/about_data
https://github.com/CityOfNewYork/nyc-geo-metadata/blob/main/Metadata/Metadata_DigitalElevationModel.md

```
python -m los_analyzer.dem.preprocess_dem /mnt/dem/city_raw/DEM_LiDAR_1ft_2010_Improved_NYC_int.tif /mnt/dem/preprocessed/
```
