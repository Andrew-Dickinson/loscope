#!/usr/bin/env bash
set -euo pipefail

if [[ -z "${OUTPUT_S3_BUCKET:-}" || -z "${OUTPUT_S3_PREFIX:-}" ]] && [[ -z "${OUTPUT_SCP_URI:-}" ]]; then
    echo "Error: no upload destination configured." >&2
    echo "  Set OUTPUT_S3_BUCKET and OUTPUT_S3_PREFIX for S3 upload, or OUTPUT_SCP_URI for SCP upload." >&2
    exit 1
fi

if [[ -z "${SOCRATA_API_KEY_ID:-}" || -z "${SOCRATA_SECRET_KEY:-}" ]]; then
    echo "Error: SOCRATA_API_KEY_ID and SOCRATA_SECRET_KEY are required." >&2
    exit 1
fi

./incremental-download.sh
./incremental-preprocess.sh

if [[ -n "${OUTPUT_S3_BUCKET:-}" && -n "${OUTPUT_S3_PREFIX:-}" ]]; then
    ./incremental-upload-s3.sh "${OUTPUT_S3_BUCKET}" "${OUTPUT_S3_PREFIX}"
else
    ./incremental-upload-scp.sh "${OUTPUT_SCP_URI}"
fi