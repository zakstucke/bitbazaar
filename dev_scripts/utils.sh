#!/bin/bash

# Stop on error:
set -e

# Run commands in parallel. E.g. run_parallel "sleep 1" "sleep 1" "sleep 1"
run_parallel () {
    parallel --ungroup -j 0 ::: "$@"
}

py_install_if_missing () {
    if ! python -c "import $1" &> /dev/null; then
        echo "$1 is not installed. Installing..."
        python -m pip install $1
    fi
}

replace_text () {
    # $1: text to replace
    # $2: replacement text
    # $3: file to replace in
    awk "{sub(\"$1\",\"$2\")} {print}" $3 > temp.txt && mv temp.txt $3
}



# Has to come at the end of these files:
source ./dev_scripts/_scr_setup/setup.sh "$@"
