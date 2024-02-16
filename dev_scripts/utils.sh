#!/bin/bash

# Stop on error:
set -e

# Run commands in parallel. E.g. run_parallel "sleep 1" "sleep 1" "sleep 1"
run_parallel () {
    # --halt now,fail=1 stops all processes if any of the error
    parallel --ungroup -j 0 --halt now,fail=1 ::: "$@"
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

# If python exists and is a 3.x version, runs the command. Otherwise, runs with python3.12/3.11/3, whichever found first.
anypython () {
    # Use python by default (e.g. virtualenv) as long as its a 3.x version:
    if command -v python &> /dev/null && [[ $(python -c 'import sys; print(sys.version_info[0])') == "3" ]]; then
        python "$@"
    elif command -v python3.12 &> /dev/null; then
        python3.12 "$@"
    elif command -v python3.11 &> /dev/null; then
        python3.11 "$@"
    elif command -v python3 &> /dev/null; then
        python3 "$@"
    else
        echo "No python found."
        exit 1
    fi
}

# Uses python re.findall(), if more than one match or no matches, errors. Otherwise returns the matched substring.
# Args:
# $1: regex string, e.g. 'foo_(.*?)_ree' (make sure to use single quotes to escape the special chars)
# $2: string to search in e.g. "foo_bar_ree"
# Returns: the matched substring, e.g. "bar"
match_substring () {
    anypython ./dev_scripts/_internal/match_substring.py "$1" "$2"
}

# Has to come at the end of these files:
source ./dev_scripts/_scr_setup/setup.sh "$@"
