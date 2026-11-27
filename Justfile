set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

# Variables
project := "televisor"

# Setup
setup:
    cargo build --workspace

# Install the televisor binary to ~/.cargo/bin and repoint MCP configs at it
install:
    cargo install --path crates/cli --locked
    televisor setup --update

# Local development
run *args:
    doppler run -- cargo run -- {{args}}

# Quality
fmt:
    cargo fmt --all -- --check

fmt-fix:
    cargo fmt --all

lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Testing
test:
    cargo test --workspace

# Ignored smoke tests against a real panel (needs TELEVISOR_TEST_* secrets)
test-network:
    doppler run -- cargo test --workspace --features televisor-core/test-network -- --ignored

# Apple apps (apps/ios, apps/tvos): fetch VLCKit into apps/vendor once
apple-deps:
    apps/scripts/fetch-vlckit.sh
