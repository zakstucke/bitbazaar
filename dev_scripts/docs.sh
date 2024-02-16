#!/bin/bash
set -e # Exit on error

# Builds the nested js site:
js_sub_build () {
    rm -rf ./docs/js_ref

    # Find all the index files and pass them to typedoc:
    index_paths=$(find ./js/bitbazaar \( -name "index.ts" -o -name "index.js" -o -name "index.cjs" -o -name "index.tsx" -o -name "index.jsx" \) -exec printf "%s " {} +)

    # Typedoc needs the target package's deps to be installed:
    npm install --prefix ./js
    npx --yes typedoc@0.25.3 --out ./docs/js_ref --readme none --tsconfig ./js/tsconfig.json $index_paths
}

# Builds the nested js site:
rust_sub_build () {
    # Nightly features used for docs (e.g. feature(doc_auto_cfg))
    rm -rf ./docs/rust_ref && cargo +nightly doc --no-deps --manifest-path ./rust/Cargo.toml --target-dir ./docs/rust_ref --all-features
}

build () {

    # Nested js site:
    js_sub_build

    # Nested rust site:
    rust_sub_build


    # Build the docs locally:
    # If fails first time, run again with the weird python fallback to fix:
    pdm run -p ./docs mkdocs build || PY_DOC_FALLBACK="1" pdm run -p ./docs mkdocs build
}

serve () {
    # Nested js site:
    js_sub_build

    # Nested rust site:
    rust_sub_build


    # Use port 8080 as 8000 & 3000 are commonly used by other dev processes
    # When any of these files/folders change, rebuild the docs:
    DOCS_PASS=passwordpassword pdm run -p ./docs mkdocs serve --dev-addr localhost:8080 -w ./docs \
        -w ./py \
        -w ./js \
        -w ./py_rust \
        -w ./rust \
        -w ./CODE_OF_CONDUCT.md -w ./README.md -w ./CONTRIBUTING.md -w ./LICENSE.md -w ./mkdocs.yml -w ./docs/python_autodoc.py
}

# Has to come at the end of these files:
source ./dev_scripts/_scr_setup/setup.sh "$@"
