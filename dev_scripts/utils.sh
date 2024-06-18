#!/usr/bin/env bash

# Stop on error:
set -e

pre_till_commit () {
    ./dev_scripts/test.sh pre_till_success
    git commit "$@"
}

# Run commands in parallel. E.g. run_endless_parallel "sleep 1" "sleep 1" "sleep 1"
# - if any exit, exit all. Because this is for endless parallelism, if something goes down, the whole thing should.
# Originally used gnu-parallel line below, but caused problems in prod and with child processes:
# parallel --ungroup -j 0 --halt now,done=1 ::: "$@"
run_endless_parallel () {
    # 4.3 needed for wait -n:
    ensure_bash_version

    # Store each pid, so can kill all and their children if one fails:
    local pids=()
    local succeeded_pids=()

    # Called after error and on ctrl-c to kill any remaining processes:
    kill_unfinished() {
        # Terminate any that didn't succeed.
        # That script will send sigterm/hup first,
        # then 15 seconds later kill if still active.
        for pid in "${pids[@]}"; do
            if [[ ! ${succeeded_pids[@]} =~ $pide ]]; then
                ./dev_scripts/process.sh terminate "$pid"
            fi
        done
    }

    # Make sure to still kill background processes if e.g. ctrl-c is pressed:
    on_external_kill() {
        kill_unfinished
        exit 1
    }
    trap 'on_external_kill' INT

    # Fire off each command in the background:
    for cmd in "$@"; do
        eval "$cmd" & pid=$!
        pids+=($pid)
    done

    for cmd in "$@"; do
        # Disable exit on error temporarily, would break the inside block:
        set +e
        # Wait for ANY PID to finish
        # The || true is needed because we call "set -e" on all our scripts.
        wait -n
        exit_status=$?
        finished_pid=$!
        # Re-enable exit on error:
        set -e

        if [ $exit_status -eq 0 ]; then
            succeeded_pids+=($finished_pid)
        fi

        # In both cases of successful exit and not,
        # kill all remaining PID's and return with the code of the original.
        # Find the command so we can print its exit code:
        finished_cmd=""
        for i in "${!pids[@]}"; do
            if [ "${pids[i]}" -eq "$finished_pid" ]; then
                finished_cmd="${@:i:i+1}"
                break
            fi
        done
        echo "Cmd exited with code=$exit_status: \"$finished_cmd\". Forcefully exiting remaining commands..."
        kill_unfinished
        return $exit_status
    done
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

# Make sure redis is up and running:
ensure_redis () {
    # In ci redis should be spun up as needed for tests manually.
    if in_ci; then
        return
    fi

    if ! redis-cli ping; then
        if [ "$(uname)" == "Darwin" ]; then
            brew services start redis
        elif [ "$(expr substr $(uname -s) 1 5)" == "Linux" ]; then
            sudo systemctl restart redis-server
        fi
    fi
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

# Return a random id to use
rand_id () {
    echo $(openssl rand -hex 3)
}

run_in_new_terminal () {
    if [ "$(uname)" == "Darwin" ]; then
        osascript -e "tell application \"Terminal\" to do script \"cd $(pwd); $1\""
    else
        x-terminal-emulator -e "$1"
    fi
}

docker_stop_all_containers () {
    sudo docker stop $(sudo docker ps -a -q) 2>/dev/null || true
}

docker_delete_all_containers () {
    sudo docker rm -vf $(sudo docker ps -a -q) 2>/dev/null || true
}

docker_prune_volumes () {
    sudo docker volume prune
}

docker_delete_all_images () {
    sudo docker rmi -f $(sudo docker images -a -q) 2>/dev/null || true
}

# Has to come at the end of these files:
source ./dev_scripts/_scr_setup/setup.sh "$@"
