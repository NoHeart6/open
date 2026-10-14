#!/bin/sh
set -eu
umask 077
mkdir -p secrets
test ! -e secrets/bridge.key && test ! -e secrets/gateway.seed
openssl rand -hex 32 > secrets/bridge.key
openssl rand -base64 32 > secrets/gateway.seed
# Compose local secret mounts must be readable by the container's non-root UID.
# Place this directory under a host directory restricted to the deployment administrator.
chmod 700 secrets
chmod 644 secrets/bridge.key secrets/gateway.seed
printf '%s\n' 'Server keys generated without printing key material. Back up the gateway seed securely.'
