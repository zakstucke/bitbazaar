#!/usr/bin/env bash

# Stop on error:
set -e

# Debug+prod prep for running top-level services
_shared_run_prep () {

}


# Starts the openobserve server:
oo () {
    ZO_ROOT_USER_EMAIL="dev@dev.com" ZO_ROOT_USER_PASSWORD="pass" \
    ZO_DATA_DIR="$(pwd)/process_data/openobserve" \
    openobserve
}

# Open telemetry collector:
collector () {
    # Make sure stopped before starting again (zj SIGHUP doesn't seem to be respected by otlp_collector)
    pkill -9 otlp_collector &> /dev/null || true
    otlp_collector --config $(pwd)/opencollector.yaml
}

# Has to come at the end of these files:
source ./dev_scripts/_scr_setup/setup.sh "$@"