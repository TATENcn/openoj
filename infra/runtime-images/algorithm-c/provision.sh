#!/usr/bin/env bash
# Assemble the development-only algorithm-c Firecracker runtime without executing
# downloaded guest binaries on the host. External inputs are HTTPS-fetched,
# SHA-256 checked, and unpacked as data from sources.lock.json.

set -euo pipefail

readonly RUNTIME_DIR="$(cd "$(dirname "$0")" && pwd)"
readonly REPOSITORY_ROOT="$(cd "$RUNTIME_DIR/../../.." && pwd)"
readonly LOCK_FILE="$RUNTIME_DIR/sources.lock.json"
readonly OUTPUT_DIR="$RUNTIME_DIR/out"
readonly CACHE_DIR="${OPENOJ_RUNTIME_CACHE:-/tmp/openoj-algorithm-c-cache}"
readonly SOURCE_DATE_EPOCH=1711929600
readonly ROOTFS_BYTES=536870912
readonly ROOTFS_UUID=8d617bd3-5336-4aec-926a-1d5c12d7f009

require_tool() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "required host tool is missing: $1" >&2
    exit 1
  fi
}

for tool in bsdtar cargo curl cut file find grep install jq mke2fs mktemp mv rm sha256sum touch truncate uname unshare; do
  require_tool "$tool"
done

if [[ "$(uname -m)" != "x86_64" ]]; then
  echo "algorithm-c v0alpha1 only supports an x86_64 build host" >&2
  exit 1
fi

mkdir -p "$CACHE_DIR" "$OUTPUT_DIR/kernel" "$OUTPUT_DIR/rootfs" "$OUTPUT_DIR/agent"

download_locked() {
  local name="$1"
  local url="$2"
  local digest="$3"
  local destination="$CACHE_DIR/$name"
  local partial="$CACHE_DIR/.$name.partial.$PPID"

  if [[ -f "$destination" ]]; then
    if printf '%s  %s\n' "$digest" "$destination" | sha256sum --check --status; then
      printf '%s\n' "$destination"
      return
    fi
    echo "cached input failed SHA-256 verification: $destination" >&2
    exit 1
  fi
  if [[ "${OPENOJ_RUNTIME_OFFLINE:-0}" == "1" ]]; then
    echo "offline runtime input is missing: $destination" >&2
    exit 1
  fi

  curl --fail --silent --show-error --location --output "$partial" "$url"
  if ! printf '%s  %s\n' "$digest" "$partial" | sha256sum --check --status; then
    echo "downloaded input failed SHA-256 verification: $name" >&2
    exit 1
  fi
  mv "$partial" "$destination"
  printf '%s\n' "$destination"
}

readonly KERNEL_NAME="$(jq -er '.kernel.filename' "$LOCK_FILE")"
readonly KERNEL_URL="$(jq -er '.kernel.url' "$LOCK_FILE")"
readonly KERNEL_SHA256="$(jq -er '.kernel.sha256' "$LOCK_FILE")"
readonly MINIROOTFS_NAME="$(jq -er '.alpine.minirootfs.filename' "$LOCK_FILE")"
readonly MINIROOTFS_URL="$(jq -er '.alpine.minirootfs.url' "$LOCK_FILE")"
readonly MINIROOTFS_SHA256="$(jq -er '.alpine.minirootfs.sha256' "$LOCK_FILE")"
readonly KERNEL_CACHE="$(download_locked "$KERNEL_NAME" "$KERNEL_URL" "$KERNEL_SHA256")"
readonly MINIROOTFS_CACHE="$(download_locked "$MINIROOTFS_NAME" "$MINIROOTFS_URL" "$MINIROOTFS_SHA256")"

while IFS=$'\t' read -r package_name package_url package_sha256; do
  download_locked "$package_name" "$package_url" "$package_sha256" >/dev/null
done < <(jq -er '.alpine.packages[] | [.filename, .url, .sha256] | @tsv' "$LOCK_FILE")

cargo build \
  --manifest-path "$REPOSITORY_ROOT/Cargo.toml" \
  --release \
  --target x86_64-unknown-linux-musl \
  --package openoj-guest-agent \
  --locked

readonly AGENT_BINARY="$REPOSITORY_ROOT/target/x86_64-unknown-linux-musl/release/openoj-guest-agent"
if ! file "$AGENT_BINARY" | grep -Fq 'static-pie linked'; then
  echo "guest agent is not a static PIE executable" >&2
  exit 1
fi

readonly BUILD_DIR="$(mktemp -d /tmp/openoj-algorithm-c-build.XXXXXX)"
readonly STAGING_DIR="$BUILD_DIR/rootfs"
readonly ROOTFS_IMAGE="$BUILD_DIR/rootfs.ext4"

cleanup() {
  if [[ "$BUILD_DIR" == /tmp/openoj-algorithm-c-build.* ]]; then
    rm -rf -- "$BUILD_DIR"
  fi
}
trap cleanup EXIT

mkdir -p "$STAGING_DIR"
bsdtar --extract --preserve-permissions --no-same-owner --no-xattrs \
  --file "$MINIROOTFS_CACHE" --directory "$STAGING_DIR"

while IFS= read -r package_name; do
  bsdtar --extract --preserve-permissions --no-same-owner --no-xattrs \
    --exclude '.*' --file "$CACHE_DIR/$package_name" --directory "$STAGING_DIR"
done < <(jq -er '.alpine.packages[].filename' "$LOCK_FILE")

install -D --mode 0755 "$AGENT_BINARY" "$STAGING_DIR/usr/local/bin/openoj-guest-agent"
install -D --mode 0755 "$RUNTIME_DIR/rootfs/sbin/openoj-init" "$STAGING_DIR/sbin/openoj-init"
install -d --mode 0700 "$STAGING_DIR/work"

test -x "$STAGING_DIR/bin/busybox"
test -x "$STAGING_DIR/usr/bin/cc"
test -x "$STAGING_DIR/usr/local/bin/openoj-guest-agent"
test -x "$STAGING_DIR/sbin/openoj-init"
test -d "$STAGING_DIR/work"

find "$STAGING_DIR" -exec touch --no-dereference --date="@$SOURCE_DATE_EPOCH" {} +
truncate --size "$ROOTFS_BYTES" "$ROOTFS_IMAGE"
E2FSPROGS_FAKE_TIME="$SOURCE_DATE_EPOCH" \
  unshare --user --map-root-user -- \
  mke2fs -q -t ext4 -L openoj-alg-c -U "$ROOTFS_UUID" \
  -O '^has_journal' -E root_owner=0:0 -d "$STAGING_DIR" "$ROOTFS_IMAGE"

install --mode 0444 "$KERNEL_CACHE" "$OUTPUT_DIR/kernel/vmlinux.bin"
install --mode 0444 "$ROOTFS_IMAGE" "$OUTPUT_DIR/rootfs/rootfs.ext4"
install --mode 0555 "$AGENT_BINARY" "$OUTPUT_DIR/agent/openoj-guest-agent"

readonly KERNEL_OUTPUT_SHA256="$(sha256sum "$OUTPUT_DIR/kernel/vmlinux.bin" | cut -d ' ' -f 1)"
readonly ROOTFS_OUTPUT_SHA256="$(sha256sum "$OUTPUT_DIR/rootfs/rootfs.ext4" | cut -d ' ' -f 1)"
readonly AGENT_OUTPUT_SHA256="$(sha256sum "$OUTPUT_DIR/agent/openoj-guest-agent" | cut -d ' ' -f 1)"
readonly TOOLCHAIN_SHA256="$(jq -r '.alpine.packages | sort_by(.filename)[] | .sha256' "$LOCK_FILE" | sha256sum | cut -d ' ' -f 1)"

jq -n --slurpfile sources "$LOCK_FILE" \
  --arg kernel_sha256 "$KERNEL_OUTPUT_SHA256" \
  --arg rootfs_sha256 "$ROOTFS_OUTPUT_SHA256" \
  --arg agent_sha256 "$AGENT_OUTPUT_SHA256" \
  --arg toolchain_sha256 "$TOOLCHAIN_SHA256" \
  --argjson source_date_epoch "$SOURCE_DATE_EPOCH" \
  '{
    runtime: $sources[0].runtime,
    version: $sources[0].version,
    architecture: $sources[0].architecture,
    source_date_epoch: $source_date_epoch,
    development_only: true,
    production_eligible: false,
    rootfs_read_only: true,
    guest_network: "absent",
    images: {
      kernel: ($sources[0].kernel + {output_sha256: $kernel_sha256}),
      rootfs: ($sources[0].alpine.minirootfs + {output_sha256: $rootfs_sha256}),
      guest_agent: {
        source: "openoj workspace",
        target: "x86_64-unknown-linux-musl",
        output_sha256: $agent_sha256,
        license: "NOASSERTION"
      },
      c_toolchain: {
        packages: $sources[0].alpine.packages,
        lock_sha256: $toolchain_sha256
      }
    },
    sbom: "sbom.spdx.json",
    validation: "implemented; real KVM execution required"
  }' > "$OUTPUT_DIR/manifest.json"

jq -n --slurpfile sources "$LOCK_FILE" \
  --arg namespace "https://openoj.invalid/spdx/algorithm-c/v0alpha1" \
  '{
    spdxVersion: "SPDX-2.3",
    dataLicense: "CC0-1.0",
    SPDXID: "SPDXRef-DOCUMENT",
    name: "openoj-algorithm-c-v0alpha1-development-runtime",
    documentNamespace: $namespace,
    creationInfo: {
      created: "2024-04-01T00:00:00Z",
      creators: ["Tool: openoj-runtime-provision"]
    },
    packages: ($sources[0].alpine.packages | map({
      name: .name,
      SPDXID: ("SPDXRef-Package-" + (.name | gsub("[^A-Za-z0-9.-]"; "-"))),
      versionInfo: .version,
      downloadLocation: .url,
      filesAnalyzed: false,
      licenseConcluded: "NOASSERTION",
      licenseDeclared: .license,
      checksums: [{algorithm: "SHA256", checksumValue: .sha256}],
      supplier: "Organization: Alpine Linux",
      primaryPackagePurpose: "LIBRARY"
    }))
  }' > "$OUTPUT_DIR/sbom.spdx.json"

(
  cd "$OUTPUT_DIR"
  sha256sum \
    kernel/vmlinux.bin \
    rootfs/rootfs.ext4 \
    agent/openoj-guest-agent \
    manifest.json \
    sbom.spdx.json > manifest.sha256
)

echo "assembled development-only algorithm-c runtime in $OUTPUT_DIR"
echo "real KVM validation remains required"
