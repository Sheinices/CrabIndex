#!/bin/sh
# Container entrypoint: picks the configuration file, then starts CrabIndex.
#
# Priority: /app/config/init.yaml > /app/config/init.conf > /app/Data/init.* > /app/defaults/init.*
# The chosen file is stored in /app/config (the config volume) and /app/init.<ext> is a symlink
# to it, so edits saved from the admin panel (and the admin token/devkey generated on first
# start) land in the volume and survive container restarts.
# The alternate format is removed (init.yaml wins over init.conf).
set -eu
umask "${UMASK:-0027}"

MODE="${CONFIG_FILE_MODE:-600}"

apply_config() {
    ext="$1"
    src="$2"
    alt="conf"
    [ "$ext" = "conf" ] && alt="yaml"
    if [ "$src" != "/app/config/init.$ext" ]; then
        install -m "$MODE" "$src" "/app/config/init.$ext"
    fi
    rm -f "/app/init.$ext"
    ln -s "/app/config/init.$ext" "/app/init.$ext"
    for f in "/app/init.$alt" "/app/config/init.$alt"; do
        if [ -f "$f" ]; then
            echo "Removing alternate config: $f"
            rm -f "$f"
        fi
    done
}

if [ -f /app/config/init.yaml ]; then
    echo "Using existing configuration (init.yaml)..."
    apply_config yaml /app/config/init.yaml
elif [ -f /app/config/init.conf ]; then
    echo "Using existing configuration (init.conf)..."
    apply_config conf /app/config/init.conf
else
    echo "Initializing configuration..."
    if [ -f /app/Data/init.yaml ]; then
        apply_config yaml /app/Data/init.yaml
    elif [ -f /app/Data/init.conf ]; then
        apply_config conf /app/Data/init.conf
    elif [ -f /app/defaults/init.yaml ]; then
        apply_config yaml /app/defaults/init.yaml
    elif [ -f /app/defaults/init.conf ]; then
        apply_config conf /app/defaults/init.conf
    else
        echo "ERROR: no init.yaml / init.conf found in /app/config, /app/Data or /app/defaults" >&2
        exit 1
    fi
fi

mkdir -p /app/Data/fdb /app/Data/temp /app/Data/log /app/Data/tracks

if [ ! -x /app/crabindex ]; then
    echo "ERROR: /app/crabindex is missing or not executable" >&2
    exit 1
fi

echo "Starting CrabIndex (version: ${CRABINDEX_VERSION:-unknown}) on $(date)"
echo "Architecture: $(uname -m)"
echo "User: $(id)"

exec "$@"
