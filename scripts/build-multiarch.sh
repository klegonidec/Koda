#!/usr/bin/env bash
set -euo pipefail
: "${IMAGE:?Set IMAGE, e.g. registry.example/koda}"
: "${TAG:=dev}"
: "${OPENCODE_VERSION:?Set a tested OPENCODE_VERSION}"
docker buildx build --platform linux/amd64,linux/arm64 -t "${IMAGE}:${TAG}" --push .
docker buildx build --platform linux/amd64,linux/arm64 -f Dockerfile.opencode --build-arg "OPENCODE_VERSION=${OPENCODE_VERSION}" -t "${IMAGE}-opencode:${OPENCODE_VERSION}" --push .
