#!/bin/bash

# Stop on error:
set -e

# Starts the open telemetry collector in a docker container if not already started
ensure_collector () {
    # Define the name of the Docker container
    CONTAINER_NAME="collector_bitbazaar"

    if [ "$(./dev_scripts/utils.sh in_ci)" = "true" ]; then
        echo "In CI, not starting open telemetry collector."
    else
        # Check if the container is already running
        if [ "$(docker inspect -f '{{.State.Running}}' $CONTAINER_NAME 2>/dev/null)" = "true" ]; then
            echo "Open telemetry collector as container '$CONTAINER_NAME' already running!"
        else
            # Start the container
            echo "Starting open telemetry collector as container '$CONTAINER_NAME'..."
            # - Link the config file
            # - Link the ./logs/ directory to /logs/ in the container
            # - Collector listens for inputs from programs on 4317
            # - Runs in detached mode
            docker run --rm --name $CONTAINER_NAME \
                -v $(pwd)/opencollector.yaml:/etc/otelcol-contrib/config.yaml \
                -v $(pwd)/logs:/logs \
                -p 127.0.0.1:4317:4317 \
                -d \
                otel/opentelemetry-collector-contrib:0.94.0
        fi
    fi
}

# Starts the openobserve server to look at dev logs/traces/metrics
oo () {
    ZO_ROOT_USER_EMAIL="dev@dev.com" ZO_ROOT_USER_PASSWORD="pass" openobserve
}

# Has to come at the end of these files:
source ./dev_scripts/_scr_setup/setup.sh "$@"