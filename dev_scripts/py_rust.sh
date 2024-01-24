#!/bin/bash

# Stop on error:
set -e

ensure_venv () {
    cd ./py_rust/

    # Make sure the venv exists:
    if [ ! -d "./.venv/" ]; then
        pipx install virtualenv || true
        virtualenv .venv/ --python=python
    fi

    cd .. # this type of stuff could be fixed with hellscript

    # Activate the target venv: (runs from windows in CI too)
    if [[ "$OSTYPE" == "msys" ]]; then
        source ./py_rust/.venv/Scripts/activate
    else
        source ./py_rust/.venv/bin/activate
    fi

    ./dev_scripts/utils.sh py_install_if_missing typing-extensions
    ./dev_scripts/utils.sh py_install_if_missing maturin
    ./dev_scripts/utils.sh py_install_if_missing pyright
    ./dev_scripts/utils.sh py_install_if_missing pytest

}

# Build and install, takes the virtualenv dir with no end slash to install to as an argument, defaults to the py_rust venv
install () {
    VENV_PATH=${1-./py/.venv}

    ensure_venv

    cd ./py_rust/
    rm -rf ./target/wheels/
    maturin build --release
    cd ..

    # Activate the target venv: (runs from windows in CI too)
    if [[ "$OSTYPE" == "msys" ]]; then
        source $VENV_PATH/Scripts/activate
    else
        source $VENV_PATH/bin/activate
    fi

    # Make sure it contains pip (pdm) doesn't by default:
    python -m ensurepip
    python -m pip install ./py_rust/target/wheels/*.whl --force-reinstall
    deactivate
}

# Has to come at the end of these files:
source ./dev_scripts/_scr_setup/setup.sh "$@"
