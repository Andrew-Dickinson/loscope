# spec.md

The following specification describes an application used to determine potential obstructions to radio signals 
between pairs of buildings in a dense urban environment. The methodology is described in detail below, but 
the high level implementation is to use rasterized Lidar data to approximate physical obstructions. This lidar data 
is supplemented with additional rasterized renderings of obstructions from other data sources.

The application is broken down into modular components in compliance with software best practices 
for ease of construction, testability, etc. This document outlines the components and their interfaces

The overall flow is:
```
Part 1: Data preprocessing

NYS Lidar Survey Data
     |
     \/
Raserized tiles  
    /\
    |
Additional obstructions  <-  NYC Building Footprints
     /\
     |
NYC New Building Permits & Tax lot shapes
```

```
Part 2: Obstruction Detection

Input
  |
  \/
Step 2.1: Compute fresnel zone shape
  |
  \/
Step 2.2: Identify tiles
  |
  \/
Step 2.3: Load rasterized tiles
  |
  \/
Step 2.4: Compute Intersection
```

## Overall Constraints
This project should be implemented in Python, using libraries available on PyPi as appropriate. Prefer to use imported 
libraries rather than implementing the logic wherever possible

### Project Structure
Follow standard python conventions, packaging each component into logical modules. Use a shared virtual environment
for all components. Do not use the system interpreter

### Testing
All functionality must be unit tested. Tests should be as brief as possible to expose the functionality as described here.
All tests must contain concise explanatory documentation in the form of "When X, item Y should do Z"

### Interfaces
The interface between each component should be JSON objects (except where binary files are explicitly indicated,
for raster map tile data). Ensure each component is independent except for the defined interfaces. It is okay for them to 
share library functions and configuration files, in fact this is encouraged where it makes sense to do so.

The encoding of the rasterized datatypes matters a great deal in keeping file sizes low. These MUST be encoded as a 
binary array to reduce the size on disk to a minimum while maintaining easy indexing and access for 
matrix compute operations (non-compressed).

JSON outputs should be indented to improve readability. There may eventually be HTTP interfaces between components, but
for now, read/write inputs and outputs directly to disk

### Coordinate Systems
This project uses 2 different coordinate systems. The input endpoints for the LOS line are provided in WGS84 + EGM96
(latitude, longitude, meters altitude). These must be converted to NYS State Plane before passing to Step 2.1, using
`translate_to_nys_plane()`. All computation, data storage, and outputs are processed in the NY State Plane - Long Island
(EPSG:6539+6360) with heights in US survey feet on the EPSG:6360 vertical datum. Earth curvature effects are corrected
for analytically within Step 2.1 rather than by changing coordinate frames.

Always make sure to use the appropriate coordinate system for each step as highlighted below. An example of converting
coordinates can be found in `src/test_scripts/coordinates_test.py`

Always use US Survey feet (usft) as the standard definition for foot.

## Part 1: Data preprocessing

#### Input
An LAS file, vended by NYS, encoding the point cloud data from the 2021 NYC lidar survey. Data is already projected into
EPSG:6539+6360. An example of reading and rasterizing LAS data can be found in `src/test_scripts/las_explore.py`. 

Each file represents a 2500 usft square, which we will slice into 25 smaller tiles.

Each file has a unique identifier in the file name, like 997240 or 235. This identifier can be used to identify the
southwest corner of the lidar tile, see `file_id_to_offset()` in `src/test_scripts/las_explore.py` for the algorithm.

These file names form a grid of 2500x2500 ft tiles, and that grid ALWAYS starts at `912117` in the very south-west, with
a lower left (SW) corner of 912500,117500 (in NYS - Long island coordinates) The grid is always aligned to that 
starting value

#### Output
25 square tiles with side length 500 usft. Each tile has:

A unique identifying ID string, computed by taking the identifier of the source lidar data file and appending `_XY`, where
XY is a grid index within the 5x5 square tile grid formed by slicing up the source lidar tile. E.g. `_04` would be the 
north-west corner of the source lidar tile

An integer x, y offset representing the position of the bottom left corner (SW) of the tile (in the NYS coordinate frame)

A 1 usft raster grid of points, where each point is a 16 bit unsigned integer which encodes the integer height of 
the lidar data at that location, in inches, using the zero point of the EPSG:6360 datum.

A list of UUID identifiers which point to “additional obstructions”, for now this list will be empty

#### Implementation
Borrow heavily from `src/test_scripts/las_explore.py`. Each grid point represents the maximum altitude of Lidar data recorded at that
XY position.

Filter out any points categorized as noise (classification 7 or 18). 

Fill in any gaps in the rasterized output using a median filter, but DO NOT apply that filter on pixels that have at
least one data point.

#### More Detail on Additional Obstructions

For now, we won't implement these, but additional obstructions will eventually have the following structure:

An identifying uuid

An “obstruction type” which is one of:
 - Existing Building Footprint
 - New Construction Permit
 - Manually Annotated

A JSON string representing arbitrary key value attributes for this obstruction 

An integer X, Y point representing the top left corner of the obstruction geometry grid, in NYS coordinates

A pair of integers H and W representing the height and width of the obstruction (measured in usft) 

The rasterized geometry of the obstruction: An H x W raster grid with 1 usft grid spacing, where each point
is a 16 bit unsigned integer which encodes the integer height of the obstruction at that location, in inches, using
the zero point of the EPSG:6360 datum.

## Part 2: Obstruction Detection
### Step 2.1: Compute fresnel zone shape
#### Input
A pair of (easting, northing, elevation) tuples for the start and end of the LOS line, already projected into
NYS State Plane - Long Island (EPSG:6539+6360). Elevation is in US survey feet using the EPSG:6360 vertical datum.
Use `translate_to_nys_plane()` to convert GPS (WGS84 + EGM96) coordinates before calling this step.

A radio frequency F, and a radius multiplier alpha, for which to generate fresnel-zone ellipsoid

#### Output
A `FresnelZone` dataclass with a width-offset encoded representation of the fresnel zone, sampled on a 1 usft grid
aligned to integer NYS state plane coordinates. The encoding stores only the valid (non-empty) extent of each
northing row, dramatically reducing memory compared to a full 2D bounding-box array.

Fields:
- `top`: uint16 array of shape `(H, maxW)`, heights in inches. Row `i` has `widths[i]` valid entries starting at
  column 0; entries beyond `widths[i]` are zero and must be ignored.
- `bottom`: uint16 array of shape `(H, maxW)`, same layout as `top`.
- `widths`: uint32 array of shape `(H,)`. Number of valid grid cells in each northing row.
- `offsets`: uint32 array of shape `(H,)`. Per-row easting offset in usft relative to `x_base_offset`.
  The easting of column `j` in row `i` is `x_base_offset + offsets[i] + j`.
- `x_base_offset`: int. Easting (usft) of the west edge of the output grid (minimum possible column start).
- `y_base_offset`: int. Northing (usft) of the south edge of the output grid (row 0).

Row `i` corresponds to northing `y_base_offset + i`. `H` is the total number of northing rows spanned by the
fresnel zone. `maxW` is the maximum width across all rows.

Heights are encoded as uint16 inches (same convention as the tile raster data). Values are clipped to [0, 65535].

#### Implementation
First compute the radius of the output fresnel zone at each point along the LOS line based on the frequency, then
scale it by multiplying the computed radius value by alpha. This lets callers set alpha=0.6 to identify if the fresnel
zone is at least 60% clear (if a first request with alpha=1 identifies an obstruction)

Compute the fresnel ellipsoid in the NYS plane using conic section / quadratic form math. Apply a spherical Earth
curvature correction along the LOS axis to account for the fact that the NYS projection does not model Earth's
curvature. Because the fresnel zone radius is much smaller than the LOS length, it is sufficient to compute the
correction factor on the LOS centerline for each row and apply it uniformly across that row's width.

### Step 2.2: Identify tiles
#### Input
The `FresnelZone` output from step 2.1, in the NYS Survey plane - Long Island (EPSG:6539+6360)

#### Output
A list of the tile identifiers created in part 1, in which any part of the fresnel zone is present. Derive tile
membership from the `FresnelZone` width-offset encoding: for each row `i`, the occupied eastings run from
`x_base_offset + offsets[i]` to `x_base_offset + offsets[i] + widths[i] - 1`

### Step 2.3: Load Rasterized tiles
#### Input
The `FresnelZone` from step 2.1 (for its `x_base_offset`, `y_base_offset`, and total extent)

The list of tile identifiers from the output of step 2.2

A list of "additional obstruction" type strings to include, or the special string '*'
(meaning all types should be used)

#### Output
A `TerrainGrid` dataclass using the same width-offset encoding as `FresnelZone`, covering the same spatial extent.

Fields:
- `heights`: uint16 array of shape `(H, maxW)`, heights in inches. Row `i` has `widths[i]` valid entries starting at
  column 0; entries beyond `widths[i]` are zero and must be ignored.
- `widths`: uint32 array of shape `(H,)`. Number of valid grid cells in each northing row.
- `offsets`: uint32 array of shape `(H,)`. Per-row easting offset in usft relative to `x_base_offset`.
- `x_base_offset`: int. Easting (usft) of the west edge of the output grid.
- `y_base_offset`: int. Northing (usft) of the south edge of the output grid (row 0).

Also include a list of the additional obstruction IDs that matched the input filter.

The `widths` and `offsets` arrays must match those of the input `FresnelZone` exactly, so that corresponding rows
and columns in the two structures refer to the same NYS coordinates without any additional alignment step.

##### Implementation
First, fill the output object with heightmap data from the referenced tiles. As we load each tile, keep track of the ID of 
all the additional obstructions referenced by that tile that match our filter from the input, keeping in mind that
one obstruction may span many tiles. 

Next, use our filtered list of obstruction IDs to load each one. Apply it to the output object by taking the maximum 
height at each grid coordinate, between the existing output data and the obstruction we loaded.

### Step 2.4: Compute Intersection
#### Input
The `FresnelZone` (with its `top`, `bottom`, `widths`, and `offsets` fields) from step 2.1

The `TerrainGrid` from step 2.3

Because both inputs share the same `widths`, `offsets`, `x_base_offset`, and `y_base_offset`, corresponding entries
refer to the same NYS coordinates and require no additional alignment.

#### Output
An `ObstructionGrid` dataclass using the same width-offset encoding as `FresnelZone` and `TerrainGrid`.

Fields:
- `values`: float32 array of shape `(H, maxW)`. Row `i` has `widths[i]` valid entries starting at column 0.
  Each value is the obstruction level on a scale of 0 to 1: how far the terrain height is above the fresnel zone
  lower bound, as a fraction of the total fresnel zone height at that location
  (`(terrain - bottom) / (top - bottom)`). Clipped to [0, 1]
- `widths`: uint32 array of shape `(H,)`. Copied from the input `FresnelZone`.
- `offsets`: uint32 array of shape `(H,)`. Copied from the input `FresnelZone`.
- `x_base_offset`: int. Copied from the input `FresnelZone`.
- `y_base_offset`: int. Copied from the input `FresnelZone`.
