use pyo3::prelude::*;

mod add;

use add::py_add;

pub fn build_module(py: Python) -> PyResult<&PyModule> {
    let m = PyModule::new(py, "utils")?;

    m.add_function(wrap_pyfunction!(py_add, m)?)?;

    Ok(m)
}
