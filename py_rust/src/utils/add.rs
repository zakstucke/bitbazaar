use pyo3::prelude::*;

#[pyfunction]
#[pyo3(name = "add")]
pub fn py_add(a: f64, b: f64) -> f64 {
    a + b
}
