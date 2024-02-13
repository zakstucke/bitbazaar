#!/bin/bash

# Stop on error:
set -e

# Pass in the version number
_install_yaml_fmt () {
    echo "Installing yamlfmt version $1..."

    # Download and make name generic across OS and arch:
    mkdir -p ./yamlfmt_installer
    curl -fsSL -o ./yamlfmt_installer/yamlfmt.tar.gz "https://github.com/google/yamlfmt/releases/download/v$1/yamlfmt_$1_$(uname -s)_$(uname -m).tar.gz"
    # Extract:
    tar -xzf ./yamlfmt_installer/yamlfmt.tar.gz -C ./yamlfmt_installer/
    # Install:
    sudo mv ./yamlfmt_installer/yamlfmt /usr/local/bin
    # Cleanup:
    rm -rf ./yamlfmt_installer/

    echo "yamlfmt version $1 installed!"
}

_install_openobserve() {
    echo "Installing openobserve version $1..."

    # os lowercase:
    OS=$(uname -s | tr '[:upper:]' '[:lower:]')
    ARCH=$(uname -m)

    echo "Downloading openobserve version $1 for $OS-$ARCH..."
    curl -L https://github.com/openobserve/openobserve/releases/download/v$1/openobserve-v$1-$OS-$ARCH.tar.gz -o openobserve.tar.gz -f
    tar -xzf openobserve.tar.gz
    rm openobserve.tar.gz
    chmod +x openobserve
    sudo mv openobserve /usr/local/bin
}

_ensure_openobserve() {
    req_ver="$1"

    if [[ -z "$req_ver" ]]; then
        echo "openobserve version not provided!"
        exit 1
    fi

    if version=$(openobserve --version 2>/dev/null); then
        # Will be "openobserve v$ver", make sure starts with "openobserve v" and remove that:
        if [[ ! "$version" =~ ^openobserve\ v ]]; then
            echo "openobserve version not found in expected format, expected 'openobserve vx.x.x', got '$version'!"
            exit 1
        fi

        # Strip prefix:
        version=${version#openobserve v}

        if [[ "$version" == "$req_ver" ]]; then
            echo "openobserve already installed with correct version $version!"
        else
            echo "openobserve incorrect version, upgrading to $version..."
            _install_openobserve $req_ver
        fi
    else
        _install_openobserve $req_ver
    fi
}

_install_biome () {
    echo "Installing biome version $1..."

    # os lowercase:
    OS=$(uname -s | tr '[:upper:]' '[:lower:]')
    ARCH=$(uname -m)

    echo "Downloading biome version $1 for $OS-$ARCH..."
    curl -L https://github.com/biomejs/biome/releases/download/cli%2Fv$1/biome-$OS-$ARCH -o biome -f
    chmod +x biome
    sudo mv biome /usr/local/bin
}

_ensure_biome() {
    req_ver="$1"

    if [[ -z "$req_ver" ]]; then
        echo "biome version not provided!"
        exit 1
    fi

    if version=$(biome --version 2>/dev/null); then
        # Will be "Version: $ver", make sure starts with "Version: " and remove that:
        if [[ ! "$version" =~ ^Version:\  ]]; then
            echo "Biome version not found in expected format, expected 'Version: x.x.x', got '$version'!"
            exit 1
        fi

        # Strip prefix:
        version=${version#Version: }

        if [[ "$version" == "$req_ver" ]]; then
            echo "biome already installed with correct version $version!"
        else
            echo "biome incorrect version, upgrading to $version..."
            _install_biome $req_ver
        fi
    else
        _install_biome $req_ver
    fi
}

initial_setup () {
    # Install useful local directories (might be unused):
    mkdir -p ./process_data
    mkdir -p ./logs

    # Make sure zetch is installed and up to date:
    if command -v zetch > /dev/null 2>&1; then
        echo "zetch already installed"
    else
        echo "zetch could not be found, installing..."
        pipx install zetch
    fi

    # Make sure openobserve is installed for dev open telemetry logging:
    _ensure_openobserve "0.8.0"

    # Make sure biome is installed for linting and formatting various files:
    _ensure_biome "1.5.3"

    # Make sure bun installed:
    if command -v bun > /dev/null 2>&1; then
        echo "bun already installed"
    else
        echo "bun could not be found, installing..."
        curl -fsSL https://bun.sh/install | bash # for macOS, Linux, and WSL
    fi

    # Make sure yamlfmt is installed which is needed by the vscode extension:
    yamlfmt_req_ver="0.10.0"
    if version=$(yamlfmt -version 2>/dev/null); then
        if [[ "$version" == "$yamlfmt_req_ver" ]]; then
            echo "yamlfmt already installed with correct version $version!"
        else
            echo "yamlfmt incorrect version, upgrading..."
            _install_yaml_fmt $yamlfmt_req_ver
        fi
    else
        _install_yaml_fmt $yamlfmt_req_ver
    fi

    # Make sure nightly is installed as use nightly for formatting and checking:
    rustup toolchain install nightly
    # Make sure nextest is installed:
    cargo install cargo-nextest --locked

    # Install pre-commit if not already:
    pipx install pre-commit || true
    pre-commit install

    echo "Setting up docs..."
    cd docs
    # Effectively simulating pdm init but won't modify upstream pyproject.toml or use existing active venv:
    pdm venv create --force python3.12
    pdm use -i .venv/bin/python
    pdm install -G:all
    cd ..

    echo "Setting up python..."
    cd py
    # Effectively simulating pdm init but won't modify upstream pyproject.toml or use existing active venv:
    pdm venv create --force python3.12
    pdm use -i .venv/bin/python
    pdm install -G:all
    cd ..

    echo "Setting up js..."
    cd js
    npm i
    cd ..



    echo "Setting up rust backed python project..."
    ./dev_scripts/py_rust.sh ensure_venv



}

# Has to come at the end of these files:
source ./dev_scripts/_scr_setup/setup.sh "$@"
