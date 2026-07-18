# LOScope - Line-of-sight obstruction analysis for NYC radio links
[![Contributors][contributors-shield]][contributors-url]
[![Forks][forks-shield]][forks-url]
[![Stargazers][stars-shield]][stars-url]
[![Issues][issues-shield]][issues-url]
[![MIT License][license-shield]][license-url]

LOScope is a line-of-sight analyzer for [NYC Mesh](https://www.nycmesh.net/). It uses high-resolution 
[NYS LIDAR survey data](https://gis.ny.gov/elevation) (supplemented with building footprints and permit filings for
new construction) along with user-supplied high-precision antenna placement information 
to give high-confidence answers about whether a potential new link is viable.


### Built With

* [Rust](https://www.rust-lang.org/) / [Rocket](https://rocket.rs/)
* [React](https://react.dev/) / [Three.js](https://threejs.org/) via [react-three-fiber](https://github.com/pmndrs/react-three-fiber)
* [Leaflet](https://leafletjs.com/)
* [PDAL](https://pdal.io/) / [GDAL](https://gdal.org/)
* [Redis](https://redis.io/)
* [Docker Compose](https://docs.docker.com/compose/)

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