#!/bin/sh
set -eu
# Railway mounts volumes as root. Only initialize the data directory, then drop privileges.
if [ "$(id -u)" = 0 ]; then
    chown app:app /data
    chmod 700 /data
    exec su-exec app:app gateway
fi
exec gateway
