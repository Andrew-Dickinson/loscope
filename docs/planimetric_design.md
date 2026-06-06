https://github.com/CityOfNewYork/nyc-planimetrics/blob/main/Capture_Rules.md

## Misc base layer enrichment ideas

Identify "true vegetation" by subtracting out buildings, sign gantries, and 
hydrography from the point classifications. Color these pixels solid green in the preview

Darken/saturate the buildings based on footprints, so they stand out against the roads and stuff

Color Hydrography minus bridges bright blue  

Remove boats by zeroing out anything that is marked as hydrography (we need to account for bridges, 
so subtract these from hydrography first)

Use bridge outlines & heights to improve approximation for bridge obstructions (instead of using whole tile)

Using street edges (and names?) (CSCL?) to draw in a little rectangle guy (google earth "hybrid" style)