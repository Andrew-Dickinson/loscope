#!/usr/bin/env bash
set -euo pipefail

./download.sh
./preprocess.sh

if [[ -n "${OUTPUT_S3_BUCKET:-}" && -n "${OUTPUT_S3_PREFIX:-}" ]]; then
    ./upload-s3.sh "${OUTPUT_S3_BUCKET}" "${OUTPUT_S3_PREFIX}"
elif [[ -n "${OUTPUT_SCP_URI:-}" ]]; then
    ./upload-scp.sh "${OUTPUT_SCP_URI}"
else
    echo "Error: no upload destination configured." >&2
    echo "  Set OUTPUT_S3_BUCKET and OUTPUT_S3_PREFIX for S3 upload, or OUTPUT_SCP_URI for SCP upload." >&2
    exit 1
fi