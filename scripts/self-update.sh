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

# Quick smoke tests — keep them side-effect free and avoid binding the real gateway port
if ! "$REPO_DIR/target/release/rune" --version >/dev/null 2>&1; then
    echo "[self-update] CLI binary version check failed — aborting"
    exit 1
fi
if ! "$REPO_DIR/target/release/rune" completion generate bash >/dev/null 2>&1; then
    echo "[self-update] CLI completion smoke check failed — aborting"
    exit 1
fi
SMOKE_CONFIG_DIR="$(mktemp -d)"
trap 'rm -rf "$SMOKE_CONFIG_DIR"' EXIT
cat > "$SMOKE_CONFIG_DIR/config.toml" <<'CFG'
[gateway]
host = "127.0.0.1"
port = 0

[database]
backend = "sqlite"

[ui]
enabled = false

[browser]
enabled = false
CFG
status=0
timeout 15s "$BINARY" --config "$SMOKE_CONFIG_DIR/config.toml" >/dev/null 2>&1 || status=$?
if [ "$status" -ne 0 ] && [ "$status" -ne 124 ]; then
    echo "[self-update] Gateway standalone smoke boot failed — aborting"
    exit 1
fi
SMOKE_PORT=43123
python3 - <<PYCFG
from pathlib import Path
path = Path(r"$SMOKE_CONFIG_DIR/config.toml")
text = path.read_text()
text = text.replace('port = 0', f'port = {43123}', 1)
path.write_text(text)
PYCFG
"$BINARY" --config "$SMOKE_CONFIG_DIR/config.toml" >/dev/null 2>&1 &
smoke_pid=$!
trap 'kill "$smoke_pid" >/dev/null 2>&1 || true; rm -rf "$SMOKE_CONFIG_DIR"' EXIT
if ! timeout 15s bash -lc 'until "$REPO_DIR/target/release/rune" --gateway-url "http://127.0.0.1:${SMOKE_PORT}" health >/dev/null 2>&1; do sleep 0.5; done' ; then
    echo "[self-update] CLI health smoke check failed — aborting"
    kill "$smoke_pid" >/dev/null 2>&1 || true
    exit 1
fi
kill "$smoke_pid" >/dev/null 2>&1 || true
wait "$smoke_pid" >/dev/null 2>&1 || true

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
