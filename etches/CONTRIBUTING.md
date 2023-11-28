# Contributor Guide

Thank you for your interest in improving this project.

This project is open-source under the [MIT license] and
welcomes contributions in the form of bug reports, feature requests, and pull requests.

Here is a list of important resources for contributors:

-   [Source Code](https://github.com/zakstucke/bitbazaar)
-   [Documentation](https://zakstucke.github.io/bitbazaar)
-   [Issue Tracker](https://github.com/zakstucke/bitbazaar/issues)
-   [Code of Conduct](CODE_OF_CONDUCT.md)

[mit license]: https://opensource.org/licenses/MIT

## How to report a bug

Report bugs on the [Issue Tracker](https://github.com/zakstucke/bitbazaar/issues).

When filing an issue, make sure to answer these questions:

-   Which operating system and core package versions are you using? (the applicable of rust/python/node etc)
-   Which version of this project are you using?
-   What did you do?
-   What did you expect to see?
-   What did you see instead?

The best way to get your bug fixed is to provide a test case,
and/or steps to reproduce the issue.

## How to request a feature

Request features on the [Issue Tracker](https://github.com/zakstucke/bitbazaar/issues).

## How to set up your development environment

-   Clone the repo: `git clone https://github.com/zakstucke/bitbazaar`
-   Install [`pipx`](https://pypa.github.io/pipx/)
-   `./dev_scripts/initial_setup.sh initial_setup`

### Python

-   Make sure Python 3.11+ is installed
-   Install [`PDM`](https://pdm.fming.dev/latest/#update-the-pdm-version)

### JS:

-   Install `node` 20 or greater
-   Install [`npx`](https://www.npmjs.com/package/npx) globally

### Rust-backed Python library

-   Make sure Python 3.11+ is installed
-   Install [`rust`](https://www.rust-lang.org/tools/install)

### Rust:

-   Install [`rust`](https://www.rust-lang.org/tools/install)

### Running tests

Checkout scripts in `./dev_scripts/` for how the system can be run, `test.sh` in particular.
Run the full test suite with `./dev_scripts/test.sh all`

## How to submit changes

Open a [pull request](https://github.com/zakstucke/bitbazaar/pulls) to submit changes to this project.

Your pull request needs to meet the following guidelines for acceptance:

-   `./dev_scripts/test.sh all` passes without failures or warnings.
-   Include unit tests. This project maintains 100% code coverage.
-   If your changes add functionality, update the documentation accordingly.

Feel free to submit early, though—we can always iterate on this.

It is recommended to open an issue before starting work on anything.
This will allow a chance to talk it over with the owners and validate your approach.
