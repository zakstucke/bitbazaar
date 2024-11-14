use pyo3::prelude::*;

mod add;

use add::py_add;

#[pymodule]
pub fn submodule(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(py_add, m)?)?;
    Ok(())
}
