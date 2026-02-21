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
This project uses 3 different coordinate systems. The input endpoints for the LOS line are provided in WSG84 + EGM96. 
Fresnel zone geometry is computed in an absolute coordinate frame such as ENU or ECEF. Finally, most computation, data
storage, and outputs are processed in the NY State Plane - Long Island (EPSG:6539+6360)

Always make sure to use the appropriate coordinate system for each step as highlighted below. An example of converting
coordinates can be found in `coordinates_test.py`

Always use US Survey feet (usft) as the standard definition for foot.

## Part 1: Data preprocessing

#### Input
An LAS file, vended by NYS, encoding the point cloud data from the 2021 NYC lidar survey. Data is already projected into
EPSG:6539+6360. An example of reading and rasterizing LAS data can be found in `las_explore.py`. 

Each file represents a 2500 usft square, which we will slice into 25 smaller tiles.

Each file has a unique identifier in the file name, like 997240 or 235. This identifier can be used to identify the
southwest corner of the lidar tile, see `file_id_to_offset()` in `las_explore.py` for the algorithm.

#### Output
25 square tiles with side length 500 usft. Each tile has:

A unique identifying ID string, computed by taking the identifier of the source lidar data file and appending `_XY`, where
XY is a grid index within the 5x5 square tile grid formed by slicing up the source lidar tile. E.g. `_04` would be the 
north-west corner of the source lidar tile

An integer x, y offset representing the position of the top left corner of the tile (in the NYS coordinate frame)

A 1 usft raster grid of points, where each point is a 16 bit unsigned integer which encodes the integer height of 
the lidar data at that location, in inches, using the zero point of the EPSG:6360 datum.

A list of UUID identifiers which point to “additional obstructions”, for now this list will be empty

#### Implementation
Borrow heavily from `las_explore.py`. Each grid point represents the maximum altitude of Lidar data recorded at that
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
A pair of latitude, longitude, meters altitude (WSG84 + EGM96) tuples for the start and end of the LOS line

A radio frequency F, and a radius multiplier alpha, for which to generate fresnel-zone ellipsoid

#### Output
Two 2D arrays, where each entry represents a sampled altitude value from the surface of the 1st fresnel zone, in a grid 
with spacing equal to one US survey foot. One array representing the bottom of the fresnel zone ellipsoid, 
and the other representing the top.

Also include a mask array, where 1 represents the presence of an altitude value for the fresnel zone in the main arrays
and a 0 represents no value (because the fresnel zone misses that pixel when viewed from above).

The datum for these arrays is the NYS Survey plane - Long Island (EPSG:6539+6360). Use a false northing & easting as 
an integer offset so that the arrays don't need to stretch all the way to the corner of the survey plane coordinates. Include
this offset as an output. Always subtract one tile width/height from each dimension of the offset (and also
add them to the size of the array twice), so that there is a buffer zone of one tile all the way around all four edges 
of the output array.

#### Implementation
First compute the radius of the output fresnel zone at each point along the LOS line based on the frequency, then 
scale it by multiplying the computed radius value by alpha. This lets callers set alpha=0.6 to identify if the fresnel
zone is at least 60% clear (if a first request with alpha=1 identifies an obstruction)

Compute the fresnel ellipsoid using an ENU or ECEF coordinate system, not the NYS Plane (to prevent projection 
distortions over long distances), and then convert to the NYS plane when sampling heights.

### Step 2.2: Identify tiles
#### Input
The 1-foot mask grid and offset from step 2.1, in the NYS Survey plane - Long Island (EPSG:6539+6360)

#### Output
A list of the tile identifiers created in part 1, in which any part of the fresnel zone is present according to the
input mask. Also include in the list, any tiles which are adjacent to the tiles identified via that query 
(expand by one tile in all 4 cardinal directions).

### Step 2.3: Load Rasterized tiles
#### Input
The offset and total grid size from step 2.1

The list of tile identifiers from the output of step 2.2

A list of "additional obstruction" type strings to include, or the special string '*'
(meaning all should types should be used)

#### Output
A single combined raster grid, with the same offset and size as the step 2.1 grid, where the grid's values are sourced 
from the height map in each tile (as well as the specified additional obstruction types)

Also include a mask for which grid coordinates are actually populated with values from loaded tiles and obstructions, vs
left blank

Also include a list of the additional obstruction IDs that matched the input filter

##### Implementation
First, fill the output grid with heightmap data from the referenced tiles. As we load each tile, keep track of the ID of 
all the additional obstructions referenced by that tile that match our filter from the input, keeping in mind that
one obstruction may span many tiles. 

Next, use our filtered list of obstruction IDs to load each one. Apply it to the output grid by taking the maximum 
height at each grid coordinate, between the existing grid and the obstruction.

### Step 2.4: Compute Intersection
#### Input
The two grids (and a mask) with fresnel zone upper and lower bounds from step 2.1

The combined grid from step 2.3

If generated according to this spec, these inputs are already in the same reference frame and require no offsets

### Output
A raster grid with the same reference frame as the inputs, containing the level of obstruction at each grid location on
a scale of 0 to 1. That is, for each grid location, report how far the combined surface grid is located above the 
fresnel zone lower bound, as a fraction of the upper bound position
