#!/bin/bash
set -e # Exit on error

# Hook into live logs from a detached running container.
logs () {
    # Check if a container name is provided
    if [ $# -eq 0 ]; then
    echo "Usage: $0 CONTAINER_NAME"
    exit 1
    fi

    # Retrieve the container name
    CONTAINER_NAME=$1

    # Trap Ctrl+C to disconnect from logs
    trap 'echo "Ctrl+C pressed. Disconnecting from logs..."' INT

    # Hook into the logs of the specified container
    docker logs -f "$CONTAINER_NAME"

    # Script execution continues after disconnecting from logs
    echo "Disconnected from logs, but container is still running."
}

# Has to come at the end of these files:
source ./dev_scripts/_scr_setup/setup.sh "$@"
