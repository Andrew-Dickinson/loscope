#!/bin/bash

set -ex

if [[ -z "${AWS_ACCOUNT:-}" ]]; then
    echo "Error: AWS_ACCOUNT env var required." >&2
    exit 1
fi

export GIT_COMMIT_HASH=$(git rev-parse HEAD)
echo "Building commit hash: $GIT_COMMIT_HASH into container images"

aws ecr get-login-password --region us-east-1 | docker login --username AWS --password-stdin "${AWS_ACCOUNT}.dkr.ecr.us-east-1.amazonaws.com"
docker build --platform linux/amd64 -f Dockerfile.cache-janitor -t loscope-cache-janitor .
docker build --platform linux/amd64 -f Dockerfile.backend -t loscope-backend .
docker build --platform linux/amd64 -f Dockerfile.frontend --build-arg GIT_COMMIT_HASH -t loscope-frontend .

docker tag loscope-cache-janitor:latest "${AWS_ACCOUNT}.dkr.ecr.us-east-1.amazonaws.com/loscope-cache-janitor:latest"
docker tag loscope-backend:latest "${AWS_ACCOUNT}.dkr.ecr.us-east-1.amazonaws.com/loscope-backend:latest"
docker tag loscope-frontend:latest "${AWS_ACCOUNT}.dkr.ecr.us-east-1.amazonaws.com/loscope-frontend:latest"

docker push "${AWS_ACCOUNT}.dkr.ecr.us-east-1.amazonaws.com/loscope-cache-janitor:latest"
docker push "${AWS_ACCOUNT}.dkr.ecr.us-east-1.amazonaws.com/loscope-backend:latest"
docker push "${AWS_ACCOUNT}.dkr.ecr.us-east-1.amazonaws.com/loscope-frontend:latest"