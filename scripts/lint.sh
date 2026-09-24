#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
echo 'Checking Rust formatting...'
cargo fmt --all --check </dev/null
echo 'Running Clippy for the workspace and all targets...'
cargo clippy --workspace --all-targets --locked -- -D warnings </dev/null
