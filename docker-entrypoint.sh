#!/bin/sh
set -e

# ── Zero-config Docker entrypoint for Rune ──────────────────────
# Produces a working runtime with zero environment variables.
# All settings use sensible defaults; override via RUNE__* env vars.

# Default paths (match Dockerfile VOLUME layout)
: "${RUNE__DATABASE__DATABASE_URL:=sqlite:///data/db/rune.db}"
: "${RUNE__GATEWAY__PORT:=8787}"
: "${RUNE__GATEWAY__HOST:=0.0.0.0}"
: "${RUNE__PATHS__DATA_DIR:=/data}"
: "${RUNE__PATHS__SESSIONS_DIR:=/data/sessions}"
: "${RUNE__PATHS__MEMORY_DIR:=/data/memory}"
: "${RUNE__PATHS__MEDIA_DIR:=/data/media}"
: "${RUNE__PATHS__SKILLS_DIR:=/data/skills}"
: "${RUNE__PATHS__LOGS_DIR:=/data/logs}"
: "${RUNE__PATHS__BACKUPS_DIR:=/data/backups}"
: "${RUNE__LOGGING__LOG_LEVEL:=info}"

export RUNE__DATABASE__DATABASE_URL
export RUNE__GATEWAY__PORT
export RUNE__GATEWAY__HOST
export RUNE__PATHS__DATA_DIR
export RUNE__PATHS__SESSIONS_DIR
export RUNE__PATHS__MEMORY_DIR
export RUNE__PATHS__MEDIA_DIR
export RUNE__PATHS__SKILLS_DIR
export RUNE__PATHS__LOGS_DIR
export RUNE__PATHS__BACKUPS_DIR
export RUNE__LOGGING__LOG_LEVEL

# Auto-detect Ollama — probe common Docker network addresses
if [ -z "${RUNE__MODELS__PROVIDERS}" ]; then
    # Check if OLLAMA_HOST is set explicitly
    if [ -n "${OLLAMA_HOST}" ]; then
        echo "[entrypoint] Ollama host configured: ${OLLAMA_HOST}"
        export RUNE__MODELS__ZERO_CONFIG_OLLAMA=true
    else
        # Probe host.docker.internal (Docker Desktop) and localhost
        for candidate in "http://host.docker.internal:11434" "http://172.17.0.1:11434" "http://localhost:11434"; do
            if wget -q --spider --timeout=2 "${candidate}/api/tags" 2>/dev/null; then
                echo "[entrypoint] Auto-detected Ollama at ${candidate}"
                export OLLAMA_HOST="${candidate}"
                export RUNE__MODELS__ZERO_CONFIG_OLLAMA=true
                break
            fi
        done
    fi
fi

# Load config from /config if present
if [ -f /config/config.toml ]; then
    echo "[entrypoint] Loading config from /config/config.toml"
    export RUNE__CONFIG_FILE=/config/config.toml
fi

# Ensure data directories exist
mkdir -p /data/db /data/sessions /data/memory /data/media \
         /data/skills /data/logs /data/backups

echo "[entrypoint] Starting Rune gateway on ${RUNE__GATEWAY__HOST}:${RUNE__GATEWAY__PORT}"

# If the first arg is a flag, prepend the default command
if [ "${1#-}" != "$1" ]; then
    set -- rune-gateway "$@"
fi

# If no args, run the gateway
if [ $# -eq 0 ]; then
    exec rune-gateway
fi

exec "$@"
