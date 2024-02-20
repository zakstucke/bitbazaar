#![warn(clippy::disallowed_types)]

use pyo3::prelude::*;

mod utils;

#[pyfunction]
pub fn hello() -> String {
    "Hello, World!".to_string()
}

/// A Python module implemented in Rust. The name of this function must match
/// the `lib.name` setting in the `Cargo.toml`, else Python will not be able to
/// import the module.
#[pymodule]
#[pyo3(name = "_rs")]
fn root_module(py: Python, m: &PyModule) -> PyResult<()> {
    // A top level function:
    m.add_function(wrap_pyfunction!(hello, m)?)?;

    // A submodule:
    m.add_submodule(utils::build_module(py)?)?;

    Ok(())
}
