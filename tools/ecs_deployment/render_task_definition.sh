#!/bin/bash

set -e

if [[ -z "${1:-}" && -z "${AWS_ACCOUNT:-}" ]]; then
    echo "Usage: $0 <aws-account-id>" >&2
    echo "   or: AWS_ACCOUNT=<id> $0" >&2
    exit 1
fi

AWS_ACCOUNT="${1:-$AWS_ACCOUNT}" \
    envsubst '${AWS_ACCOUNT}' < "$(dirname "$0")/task-definition.json"