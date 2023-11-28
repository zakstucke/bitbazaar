# BitBazaar

[![License](https://img.shields.io/badge/License-MIT-green.svg)][license]
[![Documentation](https://img.shields.io/badge/Documentation-8A2BE2)](https://zakstucke.github.io/bitbazaar)

[license]: https://github.com/zakstucke/bitbazaar/blob/main/LICENSE.md

[![PyPI](https://img.shields.io/pypi/v/bitbazaar.svg)][pypi status]
[![Status](https://img.shields.io/pypi/status/bitbazaar.svg)][pypi status]
[![Python Version](https://img.shields.io/pypi/pyversions/bitbazaar)][pypi status]
![Coverage](https://img.shields.io/badge/Coverage-100%25-green)

[pypi status]: https://pypi.org/project/bitbazaar/

[![PyPI](https://img.shields.io/pypi/v/bitbazaar_rs.svg)][pypi status]
[![Status](https://img.shields.io/pypi/status/bitbazaar_rs.svg)][pypi status]
[![Python Version](https://img.shields.io/pypi/pyversions/bitbazaar_rs)][pypi status]

[pypi status]: https://pypi.org/project/bitbazaar_rs/

An assortment of publicly available cross-language utilities useful to my projects.

## Installation

### Python

Current version: `0.0.1`

You can install _BitBazaar_ via [pip](https://pip.pypa.io/) from [PyPI](https://pypi.org/):

```console
pip install bitbazaar
```

### Javascript

Current version: `0.0.1`

You can install _BitBazaar_ via [npm](https://www.npmjs.com/):

```console
npm install bitbazaar
```

### Rust-backed Python library

Current version: `0.0.1`

You can install _BitBazaar_ via [pip](https://pip.pypa.io/) from [PyPI](https://pypi.org/):

```console
pip install bitbazaar_rs
```

Binaries are available for:

-   **Linux**: `x86_64`, `aarch64`, `i686`, `armv7l`, `musl-x86_64` & `musl-aarch64`
-   **MacOS**: `x86_64` & `arm64`
-   **Windows**: `amd64` & `win32` (NOTE: disabled currently, see build workflow for bug that needs fixing)

Otherwise, you can install from source which requires Rust stable to be installed.

### Rust

Current version: `0.0.1`

This project isn't released to a private registry store and is only accessible from github.
In below scripts replace `<ACCESS_TOKEN>` with the one supplied to you.

```yaml
# Cargo.toml

[dependencies]
# Cargo is intelligent enough to find the specific crate/Cargo.toml in the repo (Note this means 2 Cargo.toml in the same repo will break)
bitbazaar = { git = "https://<ACCESS_TOKEN>@github.com/zakstucke/bitbazaar.git", tag = "v0.0.1_rs" }
```

This installs the specific subdirectory at the target version tag (pointing to the specific commit that released that version)

## Usage

Please see the [documentation](https://zakstucke.github.io/bitbazaar) for details.

## Contributing

Contributions are very welcome.
To learn more, see the [Contributor Guide](CONTRIBUTING.md).

## License

Distributed under the terms of the [MIT license](LICENSE.md),
_BitBazaar_ is free and open source software.

## Issues

If you encounter any problems,
please [file an issue](https://github.com/zakstucke/bitbazaar/issues) along with a detailed description.
