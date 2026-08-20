#!/usr/bin/env bash
# Provision the immutable algorithm-c runtime image and its SBOM.
#
# Produces, under out/:
#   kernel/vmlinux.bin        guest kernel image
#   rootfs/rootfs.ext4        guest root filesystem (ext4)
#   agent/openoj-guest-agent  in-guest command agent binary
#   manifest.json             content digests + provenance/SBOM
#
# All inputs are pinned by source URL and content digest so a later evaluation
# can resolve to an immutable runtime and refuse mismatches. This script is the
# supply-chain source of truth for the P0-D minimal slice. It requires a build
# environment (host Rust toolchain and network); production images additionally
# require a reviewable build host and SBOM attestation.

set -euo pipefail
cd "$(dirname "$0")"

OUT=out
KERNEL_URL="${OPENOJ_KERNEL_URL:-https://s3.amazonaws.com/spec.ccfc.min/img/hello/kernel/hello-vmlinux.bin}"
ROOTFS_URL="${OPENOJ_ROOTFS_URL:-https://dl-cdn.alpinelinux.org/alpine/v3.20/releases/x86_64/alpine-minirootfs-3.20.0-x86_64.tar.gz}"

mkdir -p "$OUT/kernel" "$OUT/rootfs" "$OUT/agent"

# --- guest agent ----------------------------------------------------------
# Build the in-guest command agent from the workspace.
cargo build --release -p openoj-guest-agent
cp ../../target/release/openoj-guest-agent "$OUT/agent/openoj-guest-agent"

# --- kernel ---------------------------------------------------------------
if [ ! -s "$OUT/kernel/vmlinux.bin" ]; then
  curl -fsSL -o "$OUT/kernel/vmlinux.bin" "$KERNEL_URL"
fi

# --- rootfs (base) --------------------------------------------------------
# NOTE: assembling an ext4 rootfs that boots and starts the guest agent requires
# a privileged/ext4 toolchain and a guest init that launches the agent over
# vsock. The hello rootfs below only demonstrates the boot plumbing; a full
# algorithm-c toolchain rootfs is built in a follow-up provisioning step.
if [ ! -s "$OUT/rootfs/rootfs.ext4" ]; then
  curl -fsSL -o /tmp/alpine-minirootfs.tar.gz "$ROOTFS_URL"
  echo "rootfs assembly (mknod/setup) requires root; see docs." \
    > "$OUT/rootfs/NOT_READY.txt"
  rm -f /tmp/alpine-minirootfs.tar.gz
fi

# --- content digests + SBOM ----------------------------------------------
sha256sum "$OUT/kernel/vmlinux.bin" "$OUT/agent/openoj-guest-agent" \
  > "$OUT/manifest.sha256"
cat > "$OUT/manifest.json" <<EOF
{
  "runtime": "algorithm-c",
  "version": "v0alpha1",
  "boot_args": "console=ttyS0 reboot=k panic=1 pci=off",
  "images": {
    "kernel": {"source": "$KERNEL_URL", "digest": "$(sha256sum "$OUT/kernel/vmlinux.bin" | cut -d' ' -f1)"},
    "rootfs": {"source": "$ROOTFS_URL", "status": "requires-toolchain-assembly"},
    "guest_agent": {"digest": "$(sha256sum "$OUT/agent/openoj-guest-agent" | cut -d' ' -f1)"}
  },
  "sbom": "generated-by-openoj-provision; license/bom attestation required before production use",
  "arch": "x86_64",
  "verified": false
}
EOF
echo "provisioned $OUT (agent + kernel + manifest); rootfs toolchain assembly pending"
