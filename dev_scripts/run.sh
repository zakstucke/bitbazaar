#!/bin/bash

# Stop on error:
set -e

# Prep for running top-level services
_prep () {
    # A custom env version may have been used before, reset zetch to make sure not the case.
    zetch

    # Start open telemetry collector and openobserve in the background:
    ./dev_scripts/run.sh collector
    ./dev_scripts/run.sh oo

}


# Starts the open telemetry collector in the background to collect open telemetry data
collector () {
    if [ "$(./dev_scripts/utils.sh in_ci)" = "true" ]; then
        echo "In CI, not starting open telemetry collector."
    else
        prefix="otlp_col_"

        # Stop any current open observer processes:
        ./dev_scripts/process.sh stop $prefix

        # Run the process:
        ./dev_scripts/process.sh start "${prefix}bitbazaar" "otlp_collector --config $(pwd)/opencollector.yaml"
    fi
}

# Starts the openobserve server in the background to look at dev logs/traces/metrics
oo () {
    if [ "$(./dev_scripts/utils.sh in_ci)" = "true" ]; then
        echo "In CI, not starting openobserver."
    else
        prefix="oo_"

        # Stop any current open observer processes:
        ./dev_scripts/process.sh stop $prefix

        ZO_ROOT_USER_EMAIL="dev@dev.com" ZO_ROOT_USER_PASSWORD="pass" \
        ZO_DATA_DIR="$(pwd)/process_data/openobserve" \
            ./dev_scripts/process.sh start "${prefix}bitbazaar" "openobserve"
    fi
}

# Has to come at the end of these files:
source ./dev_scripts/_scr_setup/setup.sh "$@"