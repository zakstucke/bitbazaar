#!/usr/bin/env bash

# Stop on error:
set -e

all () {
    echo "QA..."
    ./dev_scripts/test.sh qa

    echo "Python..."
    ./dev_scripts/test.sh py -n auto

    echo "Javascript..."
    ./dev_scripts/test.sh js

    echo "Python Rust..."
    ./dev_scripts/test.sh py_rust -n auto

    echo "Rust..."
    ./dev_scripts/test.sh rust

    echo "Docs..."
    ./dev_scripts/test.sh docs


    echo "Done! All ok."
}

pre_till_success () {
    # Run pre-commit on all files repetitively until success, but break if not done in 5 gos
    index=0
    success=false

    # Trap interrupts and exit instead of continuing the loop
    trap "echo Exited!; exit;" SIGINT SIGTERM

    while [ $index -lt 5 ]; do
        index=$((index+1))
        echo "pre-commit attempt $index"
        if pre-commit run --all-files; then
            success=true
            break
        fi
    done

    if [ "$success" = true ]; then
        echo "pre-commit succeeded"
    else
        echo "pre-commit failed 5 times, something's wrong. Exiting"
        exit 1
    fi
}

# Runs pre-commit and all the static analysis stat_* functions:
qa () {
    pre_till_success

    echo "pyright..."
    ./dev_scripts/test.sh pyright
}

py () {
    cd ./py/
    # Check for COVERAGE=False/false, which is set in some workflow runs to make faster:
    if [[ "$COVERAGE" == "False" ]] || [[ "$COVERAGE" == "false" ]]; then
        echo "COVERAGE=False/false, not running coverage"
        pdm run pytest $@
    else
        pdm run pytest --cov=./bitbazaar/ $@
    fi
    cd ..
}

pyright () {
    ./dev_scripts/py_rust.sh install

    pdm run -p ./py pyright ./py_rust ./py

    echo pyright OK.
}

js () {
    cd ./js/
    bun test $@
    cd ..
}

py_rust () {
    # Build the package up to date in the specific virtualenv:
    ./dev_scripts/py_rust.sh install_debug ./py_rust/.venv

    cd py_rust

    # Activate the target venv: (runs from windows in CI too)
    if [[ "$OSTYPE" == "msys" ]]; then
        source .venv/Scripts/activate
    else
        source .venv/bin/activate
    fi

    # Have to specify to compile in debug mode (meaning it will use the install_debug call above)
    cargo nextest run --all-features
    python -m pytest $@

    deactivate
    cd ..
}

# Used internally by pre-commit:
cargo_py_rust_check () {
    # This will go through and check with no features, each feature on it's own, and all features respectively.
    # Note: won't do unnecessary checks, e.g. if no features will only run cargo check once.
    cargo hack check --manifest-path=./py_rust/Cargo.toml --each-feature
}

rust () {
    ./dev_scripts/utils.sh ensure_redis

    cargo nextest run --manifest-path ./rust/Cargo.toml --all-features $@
}

rust_bench () {
    ./dev_scripts/utils.sh ensure_redis

    cargo bench --manifest-path ./rust/Cargo.toml --all-features $@
}

# Used internally by pre-commit:
cargo_rust_check () {
    # This will go through and check with no features, each feature on it's own, and all features respectively using cargo hack.
    # Note: won't do unnecessary checks, e.g. if no features in this project will only run cargo check once.
    cargo hack check --manifest-path=./rust/Cargo.toml --each-feature
}

docs () {
    DOCS_PASS=passwordpassword ./dev_scripts/docs.sh build
}


# Has to come at the end of these files:
source ./dev_scripts/_scr_setup/setup.sh "$@"
