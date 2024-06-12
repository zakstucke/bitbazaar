#!/usr/bin/env bash

# Stop on error:
set -e

wait_for_session () {
    # Wait for session to be available, checking every 0.01 seconds, after 2 seconds break with error:
    found=false
    for i in {1..200}; do
        if zellij list-sessions | grep -q $1; then
            found=true
            # If i isn't 1, meaning literally just been created,
            # wait an extra 0.05 seconds to make sure everything is ready:
            if [ "$i" != 1 ]; then
                sleep 0.05
            fi
            break
        fi
        sleep 0.01
    done
    if [ "$found" = false ]; then
        echo "Session $1 not found after 2 seconds"
        exit 1
    fi
}

# Has to come at the end of these files:
source ./dev_scripts/_scr_setup/setup.sh "$@"
