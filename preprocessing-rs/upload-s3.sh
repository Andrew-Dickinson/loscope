#!/usr/bin/env bash
set -euo pipefail

# TODO: Test e2e on EC2

usage() {
    echo "Usage: $0 <destination-bucket> <destination-prefix>" >&2
    echo "  destination-bucket   S3 bucket name (without s3:// prefix)" >&2
    echo "  destination-prefix   Key prefix within the bucket (no leading/trailing slashes)" >&2
    exit 1
}

if [[ $# -ne 2 ]]; then
    usage
fi

DESTINATION_BUCKET=$1
DESTINATION_PREFIX=$2

ulimit -n 50000

aws configure set default.s3.max_concurrent_requests 5000
aws configure set default.s3.max_queue_size 50000
aws configure set default.s3.multipart_threshold 64MB
aws configure set default.s3.multipart_chunksize 16MB

aws s3 cp --recursive ../data/preprocessed-lidar-tiles/ "s3://${DESTINATION_BUCKET}/${DESTINATION_PREFIX}/preprocessed-lidar-tiles/"
aws s3 cp --recursive ../data/orthos/                   "s3://${DESTINATION_BUCKET}/${DESTINATION_PREFIX}/ortho-photos/"
aws s3 cp --recursive ../data/obstructions/             "s3://${DESTINATION_BUCKET}/${DESTINATION_PREFIX}/simulated-obstructions/"
aws s3 cp --recursive ../data/footprint-wkt/            "s3://${DESTINATION_BUCKET}/${DESTINATION_PREFIX}/building-footprints/"