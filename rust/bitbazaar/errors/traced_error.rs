use std::{error::Error, panic::Location};

#[cfg(feature = "axum")]
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use colored::Colorize;

use super::generic_err::GenericErr;

// Had to look at lots of different methods, this is the only thing I could get working without it getting insanely complicated and having to deriv individual types etc.
// See:
// https://users.rust-lang.org/t/getting-line-numbers-with-as-i-would-with-unwrap/47002/3
// https://stackoverflow.com/questions/74336993/getting-line-numbers-with-when-using-boxdyn-stderrorerror
// https://github.com/rust-lang/rust/issues/87401

pub struct TracedErrWrapper<T> {
    pub inner: T,
    pub location: &'static Location<'static>,
    // To prevent more complex later code, still including the into_response field, just unused, when axum isn't enabled.
    #[cfg(not(feature = "axum"))]
    pub into_response: Option<bool>,
    #[cfg(feature = "axum")]
    pub into_response: Option<fn(TracedErrWrapper<T>) -> Response>,
}

/// An error type that can be created automatically from any other error, and stores the location the error was created.
pub type TracedErr = TracedErrWrapper<Box<dyn Error + Send + 'static>>;

// Implement a display formatter for TracedErrWrapper:
impl<T: std::fmt::Display> std::fmt::Display for TracedErrWrapper<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}\n{}\n",
            format!("{}", self.location).yellow(),
            self.inner
        )?;
        Ok(())
    }
}

// Implement a debug formatter for TracedErrWrapper:
impl<T: std::fmt::Display> std::fmt::Debug for TracedErrWrapper<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            // This will usually be shown with unwrap():
            "\n{}\n{}",
            format!("{}", self.location).yellow(),
            self.inner
        )?;
        Ok(())
    }
}

/// Auto conversion to TracedErr from arbitrary errors:
impl<E: Error + Send + 'static> From<E> for TracedErr {
    #[track_caller]
    fn from(err: E) -> Self {
        TracedErrWrapper {
            inner: Box::new(err), // Store the error
            location: std::panic::Location::caller(),
            into_response: None,
        }
    }
}

impl TracedErr {
    /// Create a new TracedErr from a string.
    #[track_caller]
    pub fn from_str<S: Into<String>>(message: S) -> Self {
        TracedErrWrapper {
            inner: Box::new(GenericErr::new(message.into())),
            location: std::panic::Location::caller(),
            into_response: None,
        }
    }

    /// Axum only, create a new TracedErr from a string, and specify a function to convert it to a response if it propagates out a handler.
    /// If this isn't used, all traced errors return 500 errors.
    #[cfg(feature = "axum")]
    #[track_caller]
    pub fn from_str_with_response<S: Into<String>>(
        message: S,
        into_response: fn(TracedErr) -> Response,
    ) -> Self {
        TracedErrWrapper {
            inner: Box::new(GenericErr::new(message.into())),
            location: std::panic::Location::caller(),
            into_response: Some(into_response),
        }
    }

    /// Modify an existing err, to keep the location the same but add more information at a higher scope.
    /// * `f` - A closure that takes the current message and returns the new message. The original error location will be kept.
    ///
    /// Returns:
    /// * `Self` - The modified error, easier for replacing in an Err() statement.
    pub fn modify_msg<F>(mut self, f: F) -> Self
    where
        F: FnOnce(&str) -> String,
    {
        // If already a GenericErr, modify the message rather than creating a new error from scratch:
        if let Some(my_error) = self.inner.as_mut().downcast_mut::<GenericErr>() {
            my_error.modify_msg(f);
        } else {
            // If not already a GenericErr, make from scratch:
            self.inner = Box::new(GenericErr::new(f(format!("{}", self.inner).as_str())));
        }

        self
    }
}

// If pyo3 enabled, setup the auto traits to convert this err to a PyErr:
#[cfg(feature = "pyo3")]
use pyo3::{exceptions::PyException, prelude::*};
#[cfg(feature = "pyo3")]
impl std::convert::From<TracedErr> for PyErr {
    fn from(err: TracedErr) -> PyErr {
        // If the inner is a PyErr, return that, otherwise create a new PyException from stringifying the error:
        Python::with_gil(|py| match (*err.inner).downcast_ref::<PyErr>() {
            Some(py_err) => {
                let py_err = py_err.clone_ref(py);

                // Will be added in different ways depending on the python version:
                let msg = format!("{}", err.location);

                let final_err;

                // Notes only support 3.11 upwards:
                #[cfg(Py_3_11)]
                {
                    // Attach the location to the error as a note:
                    let value = py_err.value(py);
                    value
                        .call_method1("add_note", (format!("\n{}", msg),))
                        .expect("Failed to add note to error");
                    final_err = py_err;
                }

                // Pre 3.11 notes support, use a UserWarning exception instead:
                #[cfg(not(Py_3_11))]
                {
                    use pyo3::exceptions::PyUserWarning;

                    let wrapped = PyUserWarning::new_err((msg,));
                    wrapped.set_cause(py, Some(py_err));
                    final_err = wrapped;
                }

                final_err
            }
            // If not a py error, just display as is.
            None => PyException::new_err(format!("{}", err)),
        })
    }
}

/// When axum enabled, implement IntoResponse to return a 500 error:
#[cfg(feature = "axum")]
impl IntoResponse for TracedErr {
    fn into_response(self) -> Response {
        // Use the custom response if available, otherwise just return 500 with generic message to prevent sensitive leaks:
        if let Some(into_response) = self.into_response {
            into_response(self)
        } else {
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error.").into_response()
        }
    }
}

/// When axum enabled, implement OperationOutput so that IntoApiResponse (aide) can be used:
#[cfg(feature = "axum")]
impl aide::OperationOutput for TracedErr {
    type Inner = Self;
}
