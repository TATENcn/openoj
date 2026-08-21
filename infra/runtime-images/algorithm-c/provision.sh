#!/usr/bin/env bash
# Provision the immutable algorithm-c runtime image and its SBOM.
#
# Produces, under out/:
#   kernel/vmlinux.bin        guest kernel image (pinned source + digest)
#   rootfs/rootfs.ext4        guest root filesystem (ext4, read-only image)
#   agent/openoj-guest-agent  in-guest command agent binary (static musl)
#   manifest.json             content digests + provenance/SBOM
#
# The rootfs is built WITHOUT root or mount privileges: the Alpine minirootfs is
# extracted to a staging directory, the statically-linked guest agent and a
# minimal PID-1 init script are added, and an ext4 image is populated from that
# directory via `mke2fs -d` under `fakeroot` (no loop mount, no mknod). The
# committed rootfs image is consumed read-only by Firecracker; the guest agent
# mounts proc/sys/devtmpfs and a RAM-backed tmpfs at /work at boot.
#
# All inputs are pinned by source URL and content digest so a later evaluation
# can resolve to an immutable runtime and refuse mismatches. This script is the
# supply-chain source of truth for the P0-D minimal slice. It requires a Rust
# build environment with the `x86_64-unknown-linux-musl` target, `fakeroot`,
# `mke2fs`, `curl` and network; production images additionally require a
# reviewable build host and SBOM attestation.
#
# NOTE: the algorithm-c C toolchain (gcc/musl-dev/binutils) is NOT yet
# provisioned here; adding it needs an in-guest package install (apk under a
# fake rootfs), which is a follow-up slice. This base runtime proves the
# guest-agent vsock execution path but does NOT claim a production language
# matrix. See manifest.json:verified=false and docs/validation.

set -euo pipefail
cd "$(dirname "$0")"

OUT=out
STAGE="$(mktemp -d -t openoj-runtime-stage.XXXXXX)"
trap 'rm -rf "$STAGE"' EXIT

KERNEL_URL="${OPENOJ_KERNEL_URL:-https://s3.amazonaws.com/spec.ccfc.min/img/hello/kernel/hello-vmlinux.bin}"
KERNEL_SHA256="882fa465c43ab7d92e31bd4167da3ad6a82cb9230f9b0016176df597c6014cef"
MINIROOTFS_URL="${OPENOJ_MINIROOTFS_URL:-https://dl-cdn.alpinelinux.org/alpine/v3.20/releases/x86_64/alpine-minirootfs-3.20.0-x86_64.tar.gz}"
MINIROOTFS_SHA256="602efda518516787c716320bd46a3f50e83a74bb749e55483c2f4a9c9f8b9a38"
ROOTFS_SIZE_MIB="${OPENOJ_ROOTFS_SIZE_MIB:-64}"

mkdir -p "$OUT/kernel" "$OUT/rootfs" "$OUT/agent"

# --- tool & target guards (fail closed, never silently skip) -----------------
if ! command -v fakeroot >/dev/null 2>&1; then
  echo "error: fakeroot is required to assemble the rootfs without root" >&2
  exit 1
fi
if ! command -v mke2fs >/dev/null 2>&1; then
  echo "error: e2fsprogs (mke2fs) is required" >&2
  exit 1
fi
rustup target list --installed | grep -qx 'x86_64-unknown-linux-musl' \
  || { echo "error: run 'rustup target add x86_64-unknown-linux-musl' first" >&2; exit 1; }

# --- guest agent (static musl) ----------------------------------------------
echo "building static guest agent"
cargo build --release --target x86_64-unknown-linux-musl -p openoj-guest-agent
AGENT_BIN="$(cd ../../.. && pwd)/target/x86_64-unknown-linux-musl/release/openoj-guest-agent"
AGENT_SHA256="$(sha256sum "$AGENT_BIN" | cut -d' ' -f1)"
cp "$AGENT_BIN" "$OUT/agent/openoj-guest-agent"

# --- kernel (pinned + digest verified) --------------------------------------
if [ ! -s "$OUT/kernel/vmlinux.bin" ]; then
  echo "downloading kernel"
  curl -fsSL -o "$OUT/kernel/vmlinux.bin" "$KERNEL_URL"
fi
KERNEL_ACTUAL="$(sha256sum "$OUT/kernel/vmlinux.bin" | cut -d' ' -f1)"
if [ "$KERNEL_ACTUAL" != "$KERNEL_SHA256" ]; then
  echo "error: kernel digest mismatch (got $KERNEL_ACTUAL, want $KERNEL_SHA256)" >&2
  exit 1
fi

# --- minirootfs (pinned + digest verified) ----------------------------------
MINIROOTFS_TGZ="$(mktemp -t openoj-minirootfs.XXXXXX.tar.gz)"
trap 'rm -rf "$STAGE" "$MINIROOTFS_TGZ"' EXIT
if [ ! -s "$MINIROOTFS_TGZ" ]; then
  echo "downloading alpine minirootfs"
  curl -fsSL -o "$MINIROOTFS_TGZ" "$MINIROOTFS_URL"
fi
MINI_ACTUAL="$(sha256sum "$MINIROOTFS_TGZ" | cut -d' ' -f1)"
if [ "$MINI_ACTUAL" != "$MINIROOTFS_SHA256" ]; then
  echo "error: minirootfs digest mismatch (got $MINI_ACTUAL, want $MINIROOTFS_SHA256)" >&2
  exit 1
fi

# --- rootfs assembly (non-root) ---------------------------------------------
echo "assembling rootfs staging"
tar -xzf "$MINIROOTFS_TGZ" -C "$STAGE"

cp "$AGENT_BIN" "$STAGE/bin/openoj-guest-agent"
chmod 0755 "$STAGE/bin/openoj-guest-agent"

# PID-1 init: mount the minimal pseudo filesystems and a RAM-backed writable
# work dir, then hand control to the guest agent. The committed rootfs stays
# read-only; task artifacts live on tmpfs /work (bounded by machine memory).
cat > "$STAGE/bin/openoj-init" <<'INIT_EOF'
#!/bin/sh
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev
mkdir -p /work
mount -t tmpfs -o size=64m tmpfs /work
exec /bin/openoj-guest-agent
INIT_EOF
chmod 0755 "$STAGE/bin/openoj-init"

mkdir -p "$STAGE/work"

echo "building ext4 rootfs (${ROOTFS_SIZE_MIB}M)"
rm -f "$OUT/rootfs/rootfs.ext4"
fakeroot mke2fs -q -t ext4 -b 4096 -d "$STAGE" "$OUT/rootfs/rootfs.ext4" "${ROOTFS_SIZE_MIB}M"
ROOTFS_SHA256="$(sha256sum "$OUT/rootfs/rootfs.ext4" | cut -d' ' -f1)"

# --- content digests + SBOM -------------------------------------------------
sha256sum \
  "$OUT/kernel/vmlinux.bin" \
  "$OUT/rootfs/rootfs.ext4" \
  "$OUT/agent/openoj-guest-agent" > "$OUT/manifest.sha256"

cat > "$OUT/manifest.json" <<EOF
{
  "runtime": "algorithm-c",
  "version": "v0alpha1",
  "boot_args": "console=ttyS0 reboot=k panic=1 pci=off init=/bin/openoj-init",
  "rootfs_mode": "read-only-image; /work mounted as tmpfs by init",
  "images": {
    "kernel": {"source": "$KERNEL_URL", "sha256": "$KERNEL_ACTUAL"},
    "rootfs": {"source": "assembled-from-alpine-minirootfs-$MINIROOTFS_SHA256", "sha256": "$ROOTFS_SHA256"},
    "guest_agent": {"source": "openoj-guest-agent@musl-static", "sha256": "$AGENT_SHA256"}
  },
  "sbom": "generated-by-openoj-provision; human review and SBOM attestation required before production use",
  "arch": "x86_64",
  "toolchain": {"status": "pending", "note": "algorithm-c gcc/musl-dev assembly is a follow-up slice"},
  "verified": false
}
EOF
echo "provisioned $OUT: kernel + rootfs + static agent + manifest"
echo "  kernel $KERNEL_ACTUAL"
echo "  rootfs $ROOTFS_SHA256"
echo "  agent  $AGENT_SHA256"
echo "  toolchain: pending"
