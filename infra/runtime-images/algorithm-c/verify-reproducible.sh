#!/usr/bin/env bash

set -euo pipefail

readonly RUNTIME_DIR="$(cd "$(dirname "$0")" && pwd)"
readonly OUTPUT_DIR="$RUNTIME_DIR/out"
readonly WORK_DIR="$(mktemp -d /tmp/openoj-algorithm-c-reproducible.XXXXXX)"
readonly FIRST_OUTPUT="$WORK_DIR/first"
readonly SECOND_OUTPUT="$WORK_DIR/second"

cleanup() {
  if [[ "$WORK_DIR" == /tmp/openoj-algorithm-c-reproducible.* ]]; then
    rm -rf -- "$WORK_DIR"
  fi
}
trap cleanup EXIT

mkdir -p "$FIRST_OUTPUT" "$SECOND_OUTPUT"

bash "$RUNTIME_DIR/provision.sh"
cp -a "$OUTPUT_DIR/." "$FIRST_OUTPUT/"

bash "$RUNTIME_DIR/provision.sh"
cp -a "$OUTPUT_DIR/." "$SECOND_OUTPUT/"

diff --recursive --brief "$FIRST_OUTPUT" "$SECOND_OUTPUT"
sha256sum "$FIRST_OUTPUT/rootfs/rootfs.ext4" "$SECOND_OUTPUT/rootfs/rootfs.ext4"

echo "algorithm-c runtime repeated-build comparison passed"
