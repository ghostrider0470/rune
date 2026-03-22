#!/bin/bash
set -e
cd "$(dirname "$0")"
echo "Building UI..."
(cd ui && npm run build)
echo "Building gateway..."
cargo build --release --bin rune-gateway
echo "Starting gateway..."
./target/release/rune-gateway --config config.toml
