mod any;
mod generic_err;
mod macros;
mod test_errs;
mod traced_error;

#[cfg(feature = "axum")]
pub use traced_error::set_axum_debug;
pub use traced_error::{TracedErr, TracedResult};

pub(crate) mod prelude {
    pub use error_stack::{bail, report, Result, ResultExt};

    pub use super::any::AnyErr;
    #[allow(unused_imports)]
    pub use crate::aer;
}

#[cfg(test)]
mod tests {
    use colored::Colorize;
    use rstest::*;

    use super::*;

    #[rstest]
    fn test_traced_err_creation() {
        // Creating in test fns to have static error paths in the tests when updating actually used files.

        // Make sure TracedErr::from_err works:
        assert_eq!(
            format!("{}", test_errs::create_err_from_err()),
            format!(
                "{}\n{}",
                "bitbazaar/errors/test_errs.rs:8:5".yellow(),
                "GenericErr: Hello world".red()
            ),
        );

        // Make sure TracedErr::from_str works:
        assert_eq!(
            format!(
                "{}",
                test_errs::create_err_from_str("Goodbye, world!".to_string())
            ),
            format!(
                "{}\n{}",
                "bitbazaar/errors/test_errs.rs:13:5".yellow(),
                "GenericErr: Goodbye, world!".red()
            ),
        );

        // Make sure macro works with strings:
        assert_eq!(
            format!(
                "{}",
                test_errs::create_err_macro_from_str("Goodbye, world!".to_string())
            ),
            format!(
                "{}\n{}",
                "bitbazaar/errors/test_errs.rs:18:5".yellow(),
                "GenericErr: Goodbye, world!".red()
            ),
        );

        // Make sure macro works with existing errors:
        assert_eq!(
            format!("{}", test_errs::create_err_macro_from_err()),
            format!(
                "{}\n{}",
                "bitbazaar/errors/test_errs.rs:23:5".yellow(),
                "GenericErr: Hello world".red()
            ),
        );
    }

    #[rstest]
    fn test_traced_err_modification() {
        // Confirm can modify msg without changing err location:
        let mut err = test_errs::create_err_from_err();
        err = err.modify_msg(|old| format!("NEW. OLD: {}", old));
        assert_eq!(
            format!("{}", err),
            format!(
                "{}\n{}",
                "bitbazaar/errors/test_errs.rs:8:5".yellow(),
                "GenericErr: NEW. OLD: Hello world".red()
            ),
        );
    }

    #[cfg(feature = "pyo3")]
    #[rstest]
    fn test_traced_err_to_py_err() {
        use pyo3::PyErr;
        let err = test_errs::create_err_from_err();
        let py_err = PyErr::from(err);
        assert_eq!(
            format!("{}", py_err),
            format!(
                "Exception: {}\n{}",
                "bitbazaar/errors/test_errs.rs:8:5".yellow(),
                "GenericErr: Hello world".red()
            )
        )
    }
}
