/// A macro for building `Report<AnyErr>` objects with string information easily.
///
/// `anyerr!()` is equivalent to `Report::new(AnyErr)`
///
/// `anyerr!("foo")` is equivalent to `Report::new(AnyErr).attach_printable("foo")`
///
/// `anyerr!("foo: {}", "bar")` is equivalent to `Report::new(AnyErr).attach_printable(format!("foo: {}", "bar"))`
#[macro_export]
macro_rules! anyerr {
    () => {{
        use error_stack::Report;
        use $crate::errors::AnyErr;

        Report::new(AnyErr)
    }};

    ($str:expr) => {{
        use error_stack::Report;
        use $crate::errors::AnyErr;

        Report::new(AnyErr).attach_printable($str)
    }};

    ($str:expr, $($arg:expr),*) => {{
        use error_stack::Report;
        use $crate::errors::AnyErr;

        Report::new(AnyErr).attach_printable(format!($str, $($arg),*))
    }};
}

/// When working in a function that cannot return a result, use this to auto panic with the formatted error if something goes wrong.
///
/// Allows use of e.g. `?` in the block.
#[macro_export]
macro_rules! panic_on_err {
    ($closure:block) => {{
        use error_stack::{Result, ResultExt};
        use $crate::errors::AnyErr;

        match (|| -> Result<_, AnyErr> { $closure })() {
            Ok(s) => s,
            Err(e) => {
                panic!("{:?}", e);
            }
        }
    }};
}
