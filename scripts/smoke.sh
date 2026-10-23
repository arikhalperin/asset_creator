#!/usr/bin/env bash
# Headless, offline smoke test: open → edit → extract → export (image), and
# video → Extract Assets → review → library → sprite sheet. No window, no
# network, no models; nothing is written outside temporary folders.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo test --release --test smoke -- --nocapture
