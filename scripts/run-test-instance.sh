#!/bin/bash
# Run a test Rune instance with isolated config/data
# Usage: ./scripts/run-test-instance.sh [--build]
#
# Test instance runs on port 18792 with data in ~/.rune-test/
# Production instance remains untouched on port 18790

set -e

RUNE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TEST_CONFIG="/home/hamza/.rune-test/config.toml"
BINARY="$RUNE_ROOT/target/release/rune-gateway"

if [[ "$1" == "--build" ]]; then
    echo "Building rune-gateway..."
    cargo build --release --bin rune-gateway --manifest-path "$RUNE_ROOT/Cargo.toml"
fi

if [[ ! -f "$BINARY" ]]; then
    echo "Binary not found at $BINARY — run with --build first"
    exit 1
fi

echo "Starting test instance on port 18792..."
echo "Config: $TEST_CONFIG"
echo "Data:   ~/.rune-test/"
echo ""
echo "Production instance (port 18790) is unaffected."
echo "Press Ctrl+C to stop."
echo ""

exec "$BINARY" --config "$TEST_CONFIG" --yolo
