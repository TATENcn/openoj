#!/usr/bin/env bash

set -euo pipefail

readonly RUNTIME_DIR="$(cd "$(dirname "$0")" && pwd)"
readonly LOCK_FILE="$RUNTIME_DIR/sources.lock.json"

bash -n "$RUNTIME_DIR/provision.sh"
bash -n "$RUNTIME_DIR/verify-reproducible.sh"
bash -n "$RUNTIME_DIR/rootfs/sbin/openoj-init"

jq -e '
  .runtime == "algorithm-c" and
  .version == "v0alpha1" and
  .architecture == "x86_64" and
  (.kernel.url | startswith("https://")) and
  (.kernel.sha256 | test("^[0-9a-f]{64}$")) and
  (.alpine.minirootfs.url | startswith("https://")) and
  (.alpine.minirootfs.sha256 | test("^[0-9a-f]{64}$")) and
  (.alpine.packages | length > 0) and
  ([.alpine.packages[].filename] | length == (unique | length)) and
  all(.alpine.packages[];
    (.url | startswith("https://")) and
    (.sha256 | test("^[0-9a-f]{64}$")) and
    (.license | length > 0)
  )
' "$LOCK_FILE" >/dev/null

grep -Fq 'OPENOJ_RUNTIME_OFFLINE' "$RUNTIME_DIR/provision.sh"
grep -Fq 'x86_64-unknown-linux-musl' "$RUNTIME_DIR/provision.sh"
grep -Fq 'delete=atime,delete=ctime' "$RUNTIME_DIR/provision.sh"
grep -Fq 'hash_seed="$ROOTFS_HASH_SEED"' "$RUNTIME_DIR/provision.sh"
grep -Fq 'guest_network: "absent"' "$RUNTIME_DIR/provision.sh"
grep -Fq 'install -d --mode 0700 "$STAGING_DIR/work"' "$RUNTIME_DIR/provision.sh"
grep -Fq "grep -qs ' /dev devtmpfs ' /proc/mounts" "$RUNTIME_DIR/rootfs/sbin/openoj-init"
grep -Fq 'size=67108864' "$RUNTIME_DIR/rootfs/sbin/openoj-init"

echo "algorithm-c runtime structure checks passed"
