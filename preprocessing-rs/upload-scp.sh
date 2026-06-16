#!/usr/bin/env bash
set -euo pipefail

# TODO: Test me

usage() {
    echo "Usage: $0 <destination-path>" >&2
    echo "  destination-path   Fully qualified path including host but no trailing slash (i.e. 127.0.0.1:/my/foo/path)" >&2
    exit 1
}

if [[ $# -ne 1 ]]; then
    usage
fi

DESTINATION_PATH=$1

scp -r ../data/preprocessed-lidar-tiles/ "${DESTINATION_PATH}/preprocessed-lidar-tiles/"
scp -r ../data/orthos/ "${DESTINATION_PATH}/ortho-photos/"
scp -r ../data/obstructions/ "${DESTINATION_PATH}/simulated-obstructions/"
scp -r ../data/footprint-wkt/ "${DESTINATION_PATH}/building-footprints/"