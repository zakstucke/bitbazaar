#![allow(clippy::module_inception)]
#![allow(clippy::type_complexity)]
#![warn(clippy::disallowed_types)]

use colored::Colorize;
use pyo3::{prelude::*, wrap_pymodule};

mod utils;

#[pyfunction]
pub fn hello() -> String {
    "Hello, World!".cyan().to_string()
}

/// A Python module implemented in Rust. The name of this function must match
/// the `lib.name` setting in the `Cargo.toml`, else Python will not be able to
/// import the module.
#[pymodule]
#[pyo3(name = "_rs")]
fn root_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // A top level function:
    m.add_function(wrap_pyfunction!(hello, m)?)?;

    // A submodule:
    m.add_wrapped(wrap_pymodule!(utils::submodule))?;

    Ok(())
}
