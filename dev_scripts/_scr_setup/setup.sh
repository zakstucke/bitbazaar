#!/bin/bash

# If no function name provided, print a list of all the functions:
if [ $# -eq 0 ]; then
    # prints out lines of "declare -f $func_name" so have to remove the "declare -f ":
    declare -F | sed -r 's/^declare -f //'
fi
# Allows you to call all functions from the script:
"$@"
