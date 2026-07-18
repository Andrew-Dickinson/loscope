#!/bin/bash

set -exuo pipefail

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

aws configure set default.s3.max_concurrent_requests 50
aws configure set default.s3.max_queue_size 10000
aws configure set default.s3.multipart_threshold 64MB
aws configure set default.s3.multipart_chunksize 16MB

aws s3 cp --recursive --cli-read-timeout 0 ../data/footprint-wkt/            "s3://${DESTINATION_BUCKET}/${DESTINATION_PREFIX}/building-footprints/"
aws s3 cp --recursive --cli-read-timeout 0 ../data/obstructions/             "s3://${DESTINATION_BUCKET}/${DESTINATION_PREFIX}/simulated-obstructions/"