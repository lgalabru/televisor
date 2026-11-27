#!/bin/bash
# Fetch VLCKit, the player for what AVFoundation can't open (MKV, AVI,
# odd MPEG-TS), into apps/vendor/ for both apps to link.
#
# The builds are VideoLAN's own, pinned by version and SHA-256. They are
# hundreds of megabytes with 32-bit and Intel slices nobody here runs, so
# they are thinned to arm64 and kept out of git (apps/vendor/ is ignored).
set -euo pipefail

version="3.7.3-319ed2c0-79128878"
base="https://download.videolan.org/pub/cocoapods/prod"
# name, sha256 of the release tarball
packages=(
  "MobileVLCKit 0d04059906962ddc9a7bd1ebaa12e1f9ae85eb2466116a97a2f46886dd27a0a9"
  "TVVLCKit b5f90c226ed54d9dc1c03901c60dc7749b74a53caace2c3047e4c0b7a063e46c"
)

vendor="$(cd "$(dirname "$0")/.." && pwd)/vendor"
mkdir -p "$vendor"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

for package in "${packages[@]}"; do
  read -r name sha <<<"$package"
  if [[ -f "$vendor/$name.xcframework/.version" ]] \
    && [[ "$(cat "$vendor/$name.xcframework/.version")" == "$version" ]]; then
    echo "$name $version already in apps/vendor"
    continue
  fi
  archive="$work/$name.tar.xz"
  echo "downloading $name $version"
  curl --fail --silent --show-error --location -o "$archive" "$base/$name-$version.tar.xz"
  echo "$sha  $archive" | shasum -a 256 --check --quiet
  mkdir -p "$work/$name"
  tar -xf "$archive" -C "$work/$name"
  framework="$(find "$work/$name" -maxdepth 2 -name "$name.xcframework" -type d)"

  # Keep only arm64 in every slice: devices and Apple silicon simulators.
  while IFS= read -r binary; do
    if lipo "$binary" -verify_arch arm64 2>/dev/null; then
      if [[ "$(lipo -archs "$binary")" != "arm64" ]]; then
        lipo "$binary" -thin arm64 -output "$binary.thin"
        mv "$binary.thin" "$binary"
      fi
      # Old releases carry bitcode, which current Xcode refuses to sign.
      xcrun bitcode_strip -r "$binary" -o "$binary.stripped" && mv "$binary.stripped" "$binary"
    fi
  done < <(find "$framework" -path "*.framework/$name" -type f)

  # Debug symbols are 90 percent of the download and only symbolicate
  # crashes inside VLC; drop them and the manifest entries naming them.
  find "$framework" -name dSYMs -type d -prune -exec rm -rf {} +
  for index in 0 1 2 3; do
    /usr/libexec/PlistBuddy -c "Delete :AvailableLibraries:$index:DebugSymbolsPath" \
      "$framework/Info.plist" 2>/dev/null || true
  done

  rm -rf "${vendor:?}/$name.xcframework"
  mv "$framework" "$vendor/"
  echo "$version" > "$vendor/$name.xcframework/.version"
  du -sh "$vendor/$name.xcframework"
done
