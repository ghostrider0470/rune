#!/usr/bin/env bash
# Self-update script for Rune gateway.
# Builds from source, validates, swaps binary, restarts service.
#
# Usage: ./scripts/self-update.sh
# Called by the agent after pushing code changes.

set -euo pipefail

REPO_DIR="$HOME/Development/rune"
SERVICE="rune-gateway.service"
BINARY="$REPO_DIR/target/release/rune-gateway"
BACKUP="$REPO_DIR/target/release/rune-gateway.bak"

cd "$REPO_DIR"

echo "[self-update] Building gateway..."
if ! cargo build --release --bin rune --bin rune-gateway 2>&1; then
    echo "[self-update] BUILD FAILED — aborting, keeping current binary"
    exit 1
fi

echo "[self-update] Build succeeded. Checking binaries..."
if [ ! -f "$BINARY" ]; then
    echo "[self-update] Binary not found at $BINARY — aborting"
    exit 1
fi
if [ ! -f "$REPO_DIR/target/release/rune" ]; then
    echo "[self-update] CLI binary not found at $REPO_DIR/target/release/rune — building both binaries is required"
    exit 1
fi

# Quick smoke tests — avoid commands that require binding the real gateway port
set +e
timeout 10 env     RUNE_GATEWAY__PORT=0     RUNE__UI__ENABLED=false     RUNE__BROWSER__ENABLED=false     "$REPO_DIR/target/release/rune-gateway" --config /nonexistent-rune-config.toml >/dev/null 2>&1
status=$?
set -e
if [ "$status" -ne 124 ]; then
    echo "[self-update] Gateway binary startup sanity check failed with status $status — aborting"
    exit 1
fi
if ! "$REPO_DIR/target/release/rune" --version >/dev/null 2>&1; then
    echo "[self-update] CLI binary version check failed — aborting"
    exit 1
fi

echo "[self-update] Building UI..."
cd "$REPO_DIR/ui"
if ! npx vite build >/dev/null 2>&1; then
    echo "[self-update] UI build failed — aborting, gateway binary is updated but UI is stale"
    cd "$REPO_DIR"
    # Don't exit — gateway binary is fine, UI can be rebuilt later
fi
cd "$REPO_DIR"

echo "[self-update] Restarting service..."
if systemctl --user is-active "$SERVICE" >/dev/null 2>&1; then
    systemctl --user restart "$SERVICE"
    echo "[self-update] Service restarted."
else
    systemctl --user start "$SERVICE"
    echo "[self-update] Service started."
fi

# Wait a moment and check it came up
sleep 3
if systemctl --user is-active "$SERVICE" >/dev/null 2>&1; then
    echo "[self-update] Service is running. Update complete."
else
    echo "[self-update] WARNING: Service failed to start. Check: systemctl --user status $SERVICE"
    exit 1
fi
