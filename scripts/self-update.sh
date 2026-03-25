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
if ! cargo build --release --bin rune-gateway 2>&1; then
    echo "[self-update] BUILD FAILED — aborting, keeping current binary"
    exit 1
fi

echo "[self-update] Build succeeded. Checking binary..."
if [ ! -f "$BINARY" ]; then
    echo "[self-update] Binary not found at $BINARY — aborting"
    exit 1
fi

# Quick smoke test — just check it can print version/help
if ! "$REPO_DIR/target/release/rune" health >/dev/null 2>&1; then
    echo "[self-update] Binary smoke test failed — aborting"
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
