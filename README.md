
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo-dark.svg">
  <img alt="LOScope" src="assets/logo-light.svg" height="72">
</picture>

[![Contributors][contributors-shield]][contributors-url]
[![Forks][forks-shield]][forks-url]
[![Stargazers][stars-shield]][stars-url]
[![Issues][issues-shield]][issues-url]
[![MIT License][license-shield]][license-url]




LOScope (pronounced "Lo-scope") is a line-of-sight analyzer for [NYC Mesh](https://www.nycmesh.net/). It uses high-resolution 
[NYS LIDAR survey data](https://gis.ny.gov/elevation) (supplemented with building footprints and permit filings for
new construction) along with user-supplied high-precision antenna placement information 
to give high-confidence answers about whether a potential new link radio is viable.

### Built With

* [Rust](https://www.rust-lang.org/) / [Rocket](https://rocket.rs/)
* [React](https://react.dev/) / [Three.js](https://threejs.org/) via [react-three-fiber](https://github.com/pmndrs/react-three-fiber)
* [Leaflet](https://leafletjs.com/)
* [PDAL](https://pdal.io/) / [GDAL](https://gdal.org/)
* [Redis](https://redis.io/)
* [Docker Compose](https://docs.docker.com/compose/)

## Architecture

### Components

* **`preprocessing-rs`** - offline batch pipeline. Downloads raw LIDAR and city GIS data, rasterizes it into height-map
  tiles, and uploads the resulting artifacts to a file server (local HTTP, remote HTTP via scp, or S3).
* **`backend-rs`** - Rocket API. Loads tiles/obstructions from the asset backend, runs the obstruction-detection
  math per request, caches results in Redis, and renders terrain/obstruction imagery.
* **`frontend-ts`** - React + Leaflet + Three.js UI. Lets a user pick two points on a map, calls the backend, and
  renders the terrain and Fresnel zone in 3D.
* **`cache-janitor`** - background process that evicts old files from the on-disk asset cache shared by backend workers.

### Coordinate Systems

Two coordinate systems are used:

* **WGS84 + EGM96** (latitude, longitude, meters altitude) - used for all external inputs (e.g. a point picked on the map).
* **NY State Plane, Long Island** (EPSG:6539 horizontal + EPSG:6360 vertical, US survey feet) - used for all internal
  storage and computation.

Input points are converted from WGS84 to NYS State Plane before any analysis is performed. Earth curvature is
corrected for analytically during Fresnel zone computation rather than by changing coordinate frames.

### Terrain Tile Format

LIDAR tiles from NYS cover a 2500x2500 usft area each and are identified by a numeric ID encoding the position of
their southwest corner. Preprocessing slices each source tile into a 5x5 grid of 500x500 usft output tiles, each
stored as a single-band `.tif` raster at 1 usft/pixel resolution, with height encoded as an unsigned 16-bit integer
in inches above the EPSG:6360 vertical datum zero point. Points classified as noise in the source LIDAR data are
filtered out, and gaps are filled with a median filter.

Additional obstructions not captured by LIDAR (existing building footprints, new construction permits) are stored
separately as smaller rasters in the same encoding, tagged with an obstruction type and arbitrary key/value metadata,
and merged on top of the LIDAR tiles at query time by taking the max height at each point.

### Obstruction Analysis

Given two endpoints and a radio frequency, the backend answers whether the link is obstructed in four steps:

1. **Compute the Fresnel zone** - build the ellipsoid geometry for the given frequency (and an optional radius
   multiplier, e.g. 0.6 to check for 60%-clearance) between the two points, in NYS State Plane coordinates.
2. **Identify tiles** - determine which terrain tiles overlap the Fresnel zone's spatial extent.
3. **Load terrain** - load the height data for those tiles, and merge in any additional obstructions matching the
   requested filter.
4. **Compute intersection** - for each point in the Fresnel zone, compare terrain height against the zone's upper
   and lower bounds to produce an obstruction fraction from 0 (clear) to 1 (fully blocked).

All four steps operate on a shared row-sparse encoding: each dataset is stored as arrays of per-row width/offset
values plus a base coordinate, so only the populated extent of each row is stored, and corresponding rows/columns
across datasets refer to the same coordinates without extra alignment.

### Asset Provider Backends

The backend fetches terrain tiles, obstructions, footprints, and orthoimagery through a single `AssetFetcher`
abstraction, configured via env vars. Exactly one of two backends is used:

* **HTTP** (`LOS_ASSET_HTTP_BASE_URL`) - fetches assets from a static file server over plain HTTP.
* **S3** (`LOS_ASSET_S3_BUCKET`) - fetches assets from an S3 bucket via the AWS SDK.

Each asset type (`ElevationTile`, `Obstruction`, `ObstructionIndex`, `BuildingFootprintWKT`, `OrthoImage`, etc.) is
mapped to its own path prefix within the backend (`LOS_TERRAIN_TILE_PREFIX`, `LOS_OBSTRUCTION_PREFIX`,
`LOS_FOOTPRINTS_PREFIX`, `LOS_ORTHOS_PREFIX`). Listing which assets of a given type exist is done by reading a
`_manifest.txt` file alongside the assets, rather than listing the bucket/directory directly.

All fetched assets pass through a `CachingAssetProvider` that stores them on local disk under
`LOCAL_ASSET_CACHE_ROOT`. A distributed mutex (Redis-backed if `REDIS_URL` is set, in-process otherwise) prevents
concurrent backend workers from fetching the same asset simultaneously. The separate `cache-janitor` service
periodically scans this same cache directory and deletes files older than `MAX_AGE_HOURS`.

## Getting Started

### Prerequisites

You'll need preprocessed terrain/obstruction artifacts before the backend can serve anything useful. Either:

* Run the preprocessing pipeline yourself (see [`docs/data_setup.md`](docs/data_setup.md) for the full walkthrough,
  including hardware requirements, a Socrata API key for city open data, and an optional Fargate-based hosted path), or
* Point the backend at an existing set of artifacts (e.g. an S3 bucket someone else already generated)

You'll also need a [MeshDB](https://github.com/nycmeshnet/meshdb) API token if you want node-number-to-rooftop
resolution to work.

### Running the App

1. Copy `.env.example` to `.env` and fill in your asset backend (HTTP or S3), Redis URL, and MeshDB token
   ```sh
   cp .env.example .env
   ```
2. Bring up the backend, frontend, Redis, and cache janitor
   ```sh
   docker compose up
   ```
3. Open `http://localhost` in a browser

## Contributing

This project open-source but closed-contribution. That means that unless you coordinate with me beforehand, 
I am extremely unlikely to merge your non-trivial PR. The complexity involved in validating such
changes is pretty high, and I am but a feeble human, trying to keep my quality standards high in the face of 
a wave of low-quality AI-generated content.

If this is a problem for you, fork the repo and do whatever you like, it's MIT licenced.

## License

Distributed under the MIT License. See `LICENSE` for more information.

## Contact

Andrew Dickinson - andrew.dickinson.0216@gmail.com

Project Link: [https://github.com/Andrew-Dickinson/loscope](https://github.com/Andrew-Dickinson/loscope)

## Acknowledgments

* [NYC Mesh](https://www.nycmesh.net/)
* [NYS GIS LIDAR Survey Data](https://gis.ny.gov/elevation)
* [NYC Open Data](https://data.cityofnewyork.us/)
* [Best-README-Template](https://github.com/othneildrew/Best-README-Template/)

[contributors-shield]: https://img.shields.io/github/contributors/Andrew-Dickinson/loscope.svg?style=for-the-badge
[contributors-url]: https://github.com/Andrew-Dickinson/loscope/graphs/contributors
[forks-shield]: https://img.shields.io/github/forks/Andrew-Dickinson/loscope.svg?style=for-the-badge
[forks-url]: https://github.com/Andrew-Dickinson/loscope/network/members
[stars-shield]: https://img.shields.io/github/stars/Andrew-Dickinson/loscope.svg?style=for-the-badge
[stars-url]: https://github.com/Andrew-Dickinson/loscope/stargazers
[issues-shield]: https://img.shields.io/github/issues/Andrew-Dickinson/loscope.svg?style=for-the-badge
[issues-url]: https://github.com/Andrew-Dickinson/loscope/issues
[license-shield]: https://img.shields.io/github/license/Andrew-Dickinson/loscope.svg?style=for-the-badge
[license-url]: https://github.com/Andrew-Dickinson/loscope/blob/main/LICENSE