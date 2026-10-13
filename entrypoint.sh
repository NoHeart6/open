#!/bin/sh
# Railway runs one image for both OpenCommander services; pick the process per service.
set -e
case "${START_CMD:-mcp}" in
  gateway) exec /usr/local/bin/gateway ;;
  *) exec node dist/server.js ;;
esac