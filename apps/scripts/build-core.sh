#!/bin/bash
# Build the Rust bridge (crates/ffi) for the platform Xcode is building, as
# a static library the app links. Run by the "Build Rust core" phase of
# both apps (apps/tvos, apps/ios).
#
# Cargo runs with a clean environment: Xcode exports SDKROOT and friends for
# tvOS, and those break the macOS build scripts and proc macros cargo
# compiles for the host along the way.
set -euo pipefail

: "${PLATFORM_NAME:?run from Xcode, or set PLATFORM_NAME}"
: "${DERIVED_FILE_DIR:?run from Xcode, or set DERIVED_FILE_DIR}"

case "$PLATFORM_NAME" in
  appletvos) target=aarch64-apple-tvos ;;
  appletvsimulator) target=aarch64-apple-tvos-sim ;;
  iphoneos) target=aarch64-apple-ios ;;
  iphonesimulator) target=aarch64-apple-ios-sim ;;
  *) echo "error: unsupported platform '$PLATFORM_NAME'" >&2; exit 1 ;;
esac
# Every Apple TV and iPhone we support, and every Apple silicon Mac, is
# arm64; x86_64 simulator slices would need more Rust targets for machines
# nobody here uses.
if [[ " ${ARCHS:-arm64} " != *" arm64 "* ]]; then
  echo "error: only arm64 is supported, Xcode asked for '${ARCHS}'" >&2
  exit 1
fi

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
# The Rust side is always built optimized: a debug rustls and serde_json
# take seconds to parse a 20 MB channel list, which no one wants to debug.
env -i \
  HOME="$HOME" \
  PATH="$HOME/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin" \
  ${DEVELOPER_DIR:+DEVELOPER_DIR="$DEVELOPER_DIR"} \
  ${TVOS_DEPLOYMENT_TARGET:+TVOS_DEPLOYMENT_TARGET="$TVOS_DEPLOYMENT_TARGET"} \
  ${IPHONEOS_DEPLOYMENT_TARGET:+IPHONEOS_DEPLOYMENT_TARGET="$IPHONEOS_DEPLOYMENT_TARGET"} \
  cargo build \
    --manifest-path "$repo_root/Cargo.toml" \
    --package televisor-ffi --lib \
    --target "$target" --release

mkdir -p "$DERIVED_FILE_DIR/rust"
cp "$repo_root/target/$target/release/libtelevisor_ffi.a" "$DERIVED_FILE_DIR/rust/"
