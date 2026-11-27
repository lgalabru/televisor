#!/bin/bash
# Embed the CLI's default account in the app bundle, so the app signs in on
# first launch with no typing on the Siri Remote. Run by the "Embed seed
# account" phase when TELEVISOR_SEED_ACCOUNT is YES.
#
# The file holds a password: it lives only in the build products, never in
# the repository. Set TELEVISOR_SEED_ACCOUNT = NO for any build you share.
set -euo pipefail

seed="$TARGET_BUILD_DIR/$UNLOCALIZED_RESOURCES_FOLDER_PATH/seed-account.json"
rm -f "$seed"
if [[ "${TELEVISOR_SEED_ACCOUNT:-NO}" != "YES" ]]; then
  exit 0
fi

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
# A machine with no CLI account still gets a working app: it asks to sign in.
if ! env -i \
  HOME="$HOME" \
  PATH="$HOME/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin" \
  ${TELEVISOR_HOST:+TELEVISOR_HOST="$TELEVISOR_HOST"} \
  ${TELEVISOR_USERNAME:+TELEVISOR_USERNAME="$TELEVISOR_USERNAME"} \
  ${TELEVISOR_PASSWORD:+TELEVISOR_PASSWORD="$TELEVISOR_PASSWORD"} \
  ${TELEVISOR_ACCOUNT:+TELEVISOR_ACCOUNT="$TELEVISOR_ACCOUNT"} \
  cargo run --quiet \
    --manifest-path "$repo_root/Cargo.toml" \
    --package televisor-ffi --bin televisor-seed -- "$seed"; then
  echo "warning: no televisor account found; the app will ask you to sign in"
fi
