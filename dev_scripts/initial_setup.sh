#!/usr/bin/env bash

# Stop on error:
set -e

# Useful for platform matching, can use like:
# if is_arm; then
#     echo "arm"
# else
#     echo "not arm"
# fi
is_arm() {
    if [ "$(uname -m)" == "arm64" ] || [ "$(uname -m)" == "aarch64" ]; then
        return 0  # Return true
    else
        return 1  # Return false
    fi
}


_ensure_zellij () {
    target_version="0.40.1"
    old_version=$(./dev_scripts/utils.sh match_substring 'zellij (.*)' "$(zellij --version 2>&1)" || echo "")
    if [ "$old_version" != "$target_version" ]; then
        echo "Installing zelliji version $target_version..."

        if [ "$(uname)" == "Darwin" ]; then
            plat="apple-darwin"
        elif [ "$(expr substr $(uname -s) 1 5)" == "Linux" ]; then
            plat="unknown-linux-musl"
        fi

        if is_arm; then
            arch="aarch64"
        else
            arch="x86_64"
        fi

        curl -L https://github.com/zellij-org/zellij/releases/download/v$target_version/zellij-$arch-$plat.tar.gz -o zellij.tar.gz -f
        tar -xzf zellij.tar.gz
        rm zellij.tar.gz
        chmod +x zellij
        sudo mv zellij /usr/local/bin
        echo "zellij version $target_version installed!"
    fi
}

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


_ensure_go () {
    if ! command -v go > /dev/null 2>&1; then
        echo "go toolchain not found, installing..."
        go_version="1.22.3"
        if is_arm; then
            arch="arm64"
        else
            arch="amd64"
        fi
        if [ "$(uname)" == "Darwin" ]; then
            plat="darwin"
        elif [ "$(expr substr $(uname -s) 1 5)" == "Linux" ]; then
            plat="linux"
        fi

        curl -L https://go.dev/dl/go${go_version}.${plat}-${arch}.tar.gz -o go_src -f
        sudo tar -C /usr/local -xzf go_src
        rm go_src
        echo "export GOPATH=~/go" >> ~/.profile && source ~/.profile
        echo "Setting PATH to include golang binaries"
        echo "export PATH='$PATH':/usr/local/go/bin:$GOPATH/bin" >> ~/.profile && source ~/.profile
    fi
}

_ensure_openobserve() {
    target_version="0.10.5"
    old_version=$(./dev_scripts/utils.sh match_substring 'openobserve v(.*)' "$(openobserve --version 2>/dev/null)" || echo "")

    if [ "$old_version" != "$target_version" ]; then
        echo "Installing openobserve version $target_version..."

        OS=$(uname -s | tr '[:upper:]' '[:lower:]')
        if is_arm; then
            ARCH="arm64"
        else
            ARCH="amd64"
        fi

        curl -L https://github.com/openobserve/openobserve/releases/download/v$target_version/openobserve-v$target_version-${OS}-${ARCH}.tar.gz -o openobserve.tar.gz -f
        tar -xzf openobserve.tar.gz
        rm openobserve.tar.gz
        chmod +x openobserve
        sudo mv openobserve /usr/local/bin
    fi
}

# We don't use the default released binary as it's 250MB!
# Instead, we compile a custom one that's only 22MB.
# We manage this by removing a bunch of features we don't need.
# For custom compilation docs, see https://opentelemetry.io/docs/collector/custom-collector/
# For a full list of components to add, see https://github.com/open-telemetry/opentelemetry-collector/blob/main/cmd/otelcorecol/builder-config.yaml
_ensure_otlp_collector () {
    target_version="0.100.0"
    install_path="$HOME/compiled_otlp_collector"
    build_path="$install_path/build"
    active_version_path="$install_path/active_version.txt"

    # If active_version_path file doesn't exist, or doesn't contain target version, need to install/reinstall:
    if [ ! -f $active_version_path ] || [ "$(cat $active_version_path)" != "$target_version" ]; then
        echo "otlp_collector version $target_version needs installing..."

        # We're compiling the otlp go project from src, hence need go:
        _ensure_go

        # Remove old artifacts:
        rm -rf $install_path
        mkdir -p $install_path
        cd $install_path

        if is_arm; then
            arch="arm64"
        else
            arch="amd64"
        fi
        if [ "$(uname)" == "Darwin" ]; then
            plat="darwin"
        elif [ "$(expr substr $(uname -s) 1 5)" == "Linux" ]; then
            plat="linux"
        fi

        # Install the builder:
        curl --proto '=https' --tlsv1.2 -fL -o ocb \
        https://github.com/open-telemetry/opentelemetry-collector/releases/download/cmd%2Fbuilder%2Fv${target_version}/ocb_${target_version}_${plat}_${arch}
        chmod +x ocb

        # Write the builder config yaml file the builder needs, this specifies which components we're actually going to build:
        printf "%s\n" "dist:" \
                        "  name: otelcol-dev" \
                        "  description: Basic OTel Collector distribution for Developers" \
                        "  output_path: ./otelcol-dev" \
                        "  otelcol_version: ${target_version}" \
                        "" \
                        "exporters:" \
                        "  - gomod: go.opentelemetry.io/collector/exporter/otlphttpexporter v${target_version}" \
                        "" \
                        "processors:" \
                        "  - gomod: go.opentelemetry.io/collector/processor/batchprocessor v${target_version}" \
                        "  - gomod: go.opentelemetry.io/collector/processor/memorylimiterprocessor v${target_version}" \
                        "" \
                        "receivers:" \
                        "  - gomod: go.opentelemetry.io/collector/receiver/otlpreceiver v${target_version}" \
                        "" > builder-config.yaml

        # Run the builder:
        go env
        ./ocb --config builder-config.yaml --verbose

        # Make the binary executable:
        chmod +x otelcol-dev/otelcol-dev
        # Move the outputted binary to /usr/local/bin and rename to "otlp_collector"
        sudo mv otelcol-dev/otelcol-dev /usr/local/bin/otlp_collector

        # Update the active version so won't re-install next time unless version changes:
        echo $target_version > $active_version_path
    fi
}

_install_biome () {
    echo "Installing biome version $1..."

    # os lowercase:
    OS=$(uname -s | tr '[:upper:]' '[:lower:]')
    ARCH=$(uname -m)

    echo "Downloading biome version $1 for ${OS}-${ARCH}..."
    curl -L https://github.com/biomejs/biome/releases/download/cli%2Fv$1/biome-${OS}-${ARCH} -o biome -f
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

_install_cargo_hack () {
    # Get host target
    host=$(rustc -Vv | grep host | sed 's/host: //')
    # Download binary and install to $HOME/.cargo/bin
    curl --proto '=https' --tlsv1.2 -fsSL https://github.com/taiki-e/cargo-hack/releases/latest/download/cargo-hack-$host.tar.gz | tar xzf - -C "$HOME/.cargo/bin"
}

_ensure_cargo_hack () {
    if version=$(./dev_scripts/utils.sh match_substring 'cargo-hack (.*)' "$(cargo hack --version)"); then
        echo "cargo-hack already installed with version $version"
    else
        echo "cargo-hack not installed, installing..."
        _install_cargo_hack
    fi
}

_ensure_gnuplot () {
    if command -v gnuplot > /dev/null 2>&1; then
        echo "gnuplot already installed"
    else
        echo "gnuplot could not be found, installing..."
        if [ "$(uname)" == "Darwin" ]; then
            brew install gnuplot
        elif [ "$(expr substr $(uname -s) 1 5)" == "Linux" ]; then
            sudo apt-get install -y gnuplot
        fi
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

    # Make sure zellij installed and correct version:
    _ensure_zellij

    # Make sure openobserve is installed for dev open telemetry logging:
    _ensure_openobserve

    # Make sure otlp collector is installed as the interface between our processes and openobserve:
    _ensure_otlp_collector

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


    # Make sure nextest is installed:
    cargo install cargo-nextest --locked
    # Make sure cargo-hack is installed:
    _ensure_cargo_hack
    # Make sure gnuplot installed for criterion benchmarks:
    _ensure_gnuplot

    # Install pre-commit if not already:
    pipx install pre-commit || true
    pre-commit install

    # Make sure pdm global cache being used to speed up installs:
    pdm config install.cache on

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

    echo "Setting up rust backed python project..."
    ./dev_scripts/py_rust.sh ensure_venv



}

# Has to come at the end of these files:
source ./dev_scripts/_scr_setup/setup.sh "$@"
