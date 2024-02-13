#!/bin/bash

# Stop on error:
set -e

# Run commands in parallel. E.g. run_parallel "sleep 1" "sleep 1" "sleep 1"
run_parallel () {
    parallel --ungroup -j 0 ::: "$@"
}

py_install_if_missing () {
    # Make a version replacing dashes with underscores for the import check:
    with_underscores=$(echo $1 | sed 's/-/_/g')
    if ! python -c "import $with_underscores" &> /dev/null; then
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

# Returns "true" if looks like in_ci, "false" otherwise:
in_ci () {
    # Check if any of the CI/CD environment variables are set
    if [ -n "$GITHUB_ACTIONS" ] || [ -n "$TRAVIS" ] || [ -n "$CIRCLECI" ] || [ -n "$GITLAB_CI" ]; then
        echo "true"
    else
        echo "false"
    fi
}

# Has to come at the end of these files:
source ./dev_scripts/_scr_setup/setup.sh "$@"
