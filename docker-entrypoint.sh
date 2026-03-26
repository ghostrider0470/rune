#!/bin/sh
set -e

# ── Zero-config Docker entrypoint for Rune ──────────────────────
# Produces a working runtime with zero environment variables.
# All settings use sensible defaults; override via RUNE__* env vars.

# Default paths (match Dockerfile VOLUME layout)
: "${RUNE__DATABASE__BACKEND:=sqlite}"
: "${RUNE__DATABASE__SQLITE_PATH:=/data/db/rune.db}"
: "${RUNE__GATEWAY__PORT:=8787}"
: "${RUNE__GATEWAY__HOST:=0.0.0.0}"
: "${RUNE__PATHS__DATA_DIR:=/data}"
: "${RUNE__PATHS__DB_DIR:=/data/db}"
: "${RUNE__PATHS__SESSIONS_DIR:=/data/sessions}"
: "${RUNE__PATHS__MEMORY_DIR:=/data/memory}"
: "${RUNE__PATHS__MEDIA_DIR:=/data/media}"
: "${RUNE__PATHS__SKILLS_DIR:=/data/skills}"
: "${RUNE__PATHS__PLUGINS_DIR:=/data/plugins}"
: "${RUNE__PATHS__LOGS_DIR:=/data/logs}"
: "${RUNE__PATHS__BACKUPS_DIR:=/data/backups}"
: "${RUNE__PATHS__CONFIG_DIR:=/config}"
: "${RUNE__PATHS__SECRETS_DIR:=/secrets}"
: "${RUNE__LOGGING__LOG_LEVEL:=info}"
: "${RUNE__UI__ENABLED:=true}"
: "${RUNE__BROWSER__ENABLED:=true}"
: "${RUNE__CHANNELS__ENABLED:=webchat}"

export RUNE__DATABASE__BACKEND
export RUNE__DATABASE__SQLITE_PATH
export RUNE__GATEWAY__PORT
export RUNE__GATEWAY__HOST
export RUNE__PATHS__DATA_DIR
export RUNE__PATHS__DB_DIR
export RUNE__PATHS__SESSIONS_DIR
export RUNE__PATHS__MEMORY_DIR
export RUNE__PATHS__MEDIA_DIR
export RUNE__PATHS__SKILLS_DIR
export RUNE__PATHS__PLUGINS_DIR
export RUNE__PATHS__LOGS_DIR
export RUNE__PATHS__BACKUPS_DIR
export RUNE__PATHS__CONFIG_DIR
export RUNE__PATHS__SECRETS_DIR
export RUNE__LOGGING__LOG_LEVEL
export RUNE__UI__ENABLED
export RUNE__BROWSER__ENABLED
export RUNE__CHANNELS__ENABLED

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


# Seed /config/config.toml on first boot so the mounted config volume is self-describing
# and survives container/image churn with a real config artifact.
if [ ! -f /config/config.toml ]; then
    cat >/config/config.toml <<EOF
mode = "standalone"

[gateway]
host = "0.0.0.0"
port = ${RUNE__GATEWAY__PORT}

[database]
backend = "sqlite"
sqlite_path = "/data/db/rune.db"
max_connections = 10
run_migrations = true

[paths]
db_dir = "/data/db"
sessions_dir = "/data/sessions"
memory_dir = "/data/memory"
media_dir = "/data/media"
skills_dir = "/data/skills"
plugins_dir = "/data/plugins"
logs_dir = "/data/logs"
backups_dir = "/data/backups"
config_dir = "/config"
secrets_dir = "/secrets"

[ui]
enabled = ${RUNE__UI__ENABLED}

[browser]
enabled = ${RUNE__BROWSER__ENABLED}

[channels]
enabled = ["webchat"]
EOF
    echo "[entrypoint] Seeded /config/config.toml with zero-config defaults"
fi

# Load config from /config if present
if [ -f /config/config.toml ]; then
    echo "[entrypoint] Loading config from /config/config.toml"
    export RUNE__CONFIG_FILE=/config/config.toml
fi

# Ensure data directories exist
mkdir -p /data/db /data/sessions /data/memory /data/media \
         /data/skills /data/plugins /data/logs /data/backups

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
