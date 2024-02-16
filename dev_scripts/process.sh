#!/bin/bash

# Stop on error:
set -e

PROCESS_PREFIX="dev_script_process"

# Start a process with this system. $1: process_name, $2: stringified command to run
# These processes will be tracked and are listable and stopable.
# All processes should stop when terminal is shut
start() {
    if [ "$#" -ne 2 ]; then
        echo "Usage: $0 <process_name> <stringified command to run>"
        return 1
    fi

    # If process_name is empty, error
    if [ -z "$1" ]; then
        echo "Process name cannot be empty!"
        return 1
    fi

    local process_name="$1"
    local process_command="$2"
    local process_id_file="/tmp/${PROCESS_PREFIX}_${process_name}.pid"

    # Check if the process is already running
    if [ -e "$process_id_file" ]; then
        local existing_pid=$(<"$process_id_file")
        if ps -p "$existing_pid" > /dev/null; then
            echo "Process '$process_name' is already running with PID $existing_pid"
            return 1
        else
            # Remove stale PID file
            rm "$process_id_file"
        fi
    fi

    # Start the process and write the processes output to $(pwd)/process_data/processes/$process_name.log
    local log_file="$(pwd)/logs/proc_$process_name.log"
    mkdir -p "$(dirname "$log_file")"
    # Clear the logfile to start from scratch:
    > "$log_file"

    # Start the process in its own process group, rerouting all output to the logfile:
    eval "$process_command" > "$log_file" 2>&1 &

    # Capture the PID of the process and write it to a file
    local new_pid=$!

    # Wait for 0.2 seconds, if the process has exited already with a non-zero exit code, then print the log file
    sleep 0.2
    if ! ps -p "$new_pid" > /dev/null; then
        cat "$log_file"
        echo "Process '$process_name' failed (wasn't running after 0.2 seconds), output in logfile printed above."
        return 1
    fi

    echo "$new_pid" > "$process_id_file"
    echo "Process '$process_name' started with PID $new_pid, output will write to '$log_file'"
}

# Stop processes with a given namespace started with this system. $1: process_name
# E.g. scr process.sh stop my_process would stop my_process and any processes with that as a prefix.
stop() {
    if [ "$#" -ne 1 ]; then
        echo "Usage: $0 <process_name>"
        return 1
    fi

    local process_name="$1"
    local process_id_files=(/tmp/${PROCESS_PREFIX}_${process_name}*.pid)

    echo "Stopping processes matching or prefixed by '$process_name'..."
    for process_id_file in "${process_id_files[@]}"; do
        if [ -e "$process_id_file" ]; then
            while IFS= read -r pid_to_kill; do
                echo "Stopping process with PID: $pid_to_kill"
                terminate "$pid_to_kill"
            done < "$process_id_file"
            rm "$process_id_file"
        fi
    done
    echo "Stopped ${#process_id_files[@]} process."
}

# List all processes actively running processes that were started with start()
list() {
    local pid_files=$(ls /tmp/${PROCESS_PREFIX}_*.pid 2>/dev/null)

    if [ -z "$pid_files" ]; then
        echo "No processes running!"
        return
    fi

    echo "Processes:"
    for pid_file in $pid_files; do
        local process_name=$(basename "$pid_file" | sed "s/${PROCESS_PREFIX}_//g" | sed "s/.pid//g")
        local pid=$(<"$pid_file")
        echo "  Process: $process_name, PID: $pid"
    done
}

# Terminate a process and all of its child processes
terminate() {
    local parent_pid="$1"
    local IS_CHILD=$2

    # Terminate the child processes of the parent PID
    local child_pids=$(pgrep -P "$parent_pid")
    for pid in $child_pids; do
        terminate "$pid" "true"
    done

    # Terminate the parent PID
    if ps -p "$parent_pid" > /dev/null; then
        if [ "$IS_CHILD" = "true" ]; then
            echo "Terminating child: $parent_pid"
        else
            echo "Terminating root: $parent_pid"
        fi
        # Or true to not error if the process is already dead:
        kill -9 "$parent_pid" > /dev/null 2>&1 || true
    fi
}

# Has to come at the end of these files:
source ./dev_scripts/_scr_setup/setup.sh "$@"