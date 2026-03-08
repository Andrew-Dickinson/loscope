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

### Tax Lots
Download here:
https://data.cityofnewyork.us/City-Government/TAX_LOT_POLYGON/i38t-6if2/about_data
- Export
- CSV
- Download

Or
```
wget "https://data.cityofnewyork.us/resource/i38t-6if2.json?\$limit=99999999999"
```

```
python tools/parse_tax_lots.py data/tax-lots/TAX_LOT_POLYGON_20260306.csv output-dir
```

Unfortunately this is only "base" lots. You need to join this with the condo
lot mappings to get a true list of all BBL polys:
https://data.cityofnewyork.us/City-Government/Digital-Tax-Map-Condominiums/p8u6-a6it/about_data



### New Building Permits (Part 1)
Download here:
https://data.cityofnewyork.us/Housing-Development/DOB-Job-Application-Filings/ic3t-wcy2/about_data
- Actions
- Query Data
- Filter: `Job type` `is one of` `NB`

Also here:
https://data.cityofnewyork.us/Housing-Development/DOB-NOW-Build-Job-Application-Filings/w9ak-ipjd/

All Certificates of occupancy issued since March 2021:
https://data.cityofnewyork.us/Housing-Development/DOB-NOW-Certificate-of-Occupancy/pkdm-hqz6/about_data

#### Construction Phases
1. Pre filing
2. Application filed / under review / changes requested
3. Application Approved
4. Permits Issued
5. Permits fully issued?
6. Sign-off
https://web.archive.org/web/20150907123328/http://www.nyc.gov/html/dob/downloads/pdf/bisjobstatus.pdf
https://www.nyc.gov/assets/buildings/pdf/job-status-types-and-codes.pdf


### Permits
https://data.cityofnewyork.us/Housing-Development/DOB-Permit-Issuance/ipu4-2q9a/about_data
https://data.cityofnewyork.us/Housing-Development/DOB-NOW-Build-Approved-Permits/rbx6-tga4/about_data


###


Craft a SQL query that uses the new tables and produces a similar output to new_construction_co.sql based on new building
job applications that have one or more construction permits that were active (between issuance and expiration) 
at any time since 2025-01-01. Use the same BBL and tax lot strategy to get the geometry, don't forget to resolve condo
billing BBLs to the base BBL

