use error_stack::Context;

/// A generic trace_stack error to use when you don't want to create custom error types.
#[derive(Debug, Default)]
pub struct AnyErr;

impl std::fmt::Display for AnyErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AnyErr")
    }
}

impl Context for AnyErr {}
