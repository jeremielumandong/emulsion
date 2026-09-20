#!/usr/bin/env bash
# Pure policy checks run on any host without creating a graphics device.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_DIR="$(mktemp -d "${TMPDIR:-/tmp}/emulsion-renderer-policy.XXXXXX")"
trap 'rm -rf "$TEST_DIR"' EXIT
for source in \
  gpui-pre-windows/src/rendering_policy.rs \
  gpui-pre-wgpu/src/adapter_policy.rs \
  gpui-pre/src/presentation_policy.rs; do
  rustc --edition 2024 --test "$ROOT_DIR/vendor/gpui/$source" -o "$TEST_DIR/policy-test"
  "$TEST_DIR/policy-test"
done
