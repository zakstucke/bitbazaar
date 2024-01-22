#[macro_export]
/// A helper for the aer! macro.
macro_rules! _aer_inner {
    ($any_err:expr) => {{
        use error_stack::{Context, Report, Result, ResultExt};

        $crate::spez! {
            for x = $any_err;

            // error_stack err -> Report<AnyErr>
            match<T: error_stack::Context> T  -> Report<AnyErr> {
                Report::new(x).change_context(AnyErr)
            }
            // Error + Sync + Send + 'static -> Report<AnyErr>
            match<T: std::error::Error + Sync + Send + 'static> T  -> Report<AnyErr> {
                Report::new(x).change_context(AnyErr)
            }
            // Report<T> -> Report<AnyErr>
            match<T> Report<T>  -> Report<AnyErr> {
                x.change_context(AnyErr)
            }
            // error_stack::Result<T, E> -> Result<T, AnyErr>
            match<T, E: Context> Result<T, E>  -> Result<T, AnyErr> {
                x.change_context(AnyErr)
            }
            // core::result::Result<T, E> -> Result<T, AnyErr>
            match<T, E: Context> core::result::Result<T, E>  -> Result<T, AnyErr> {
                x.change_context(AnyErr)
            }
            // Into<String> -> Report<AnyErr>
            match<T: Into<String>> T -> Report<AnyErr> {
                Report::new(AnyErr).attach_printable(x.into())
            }
        }
    }};
}

#[macro_export]
/// A helper for the aer! macro.
macro_rules! _aer_inner_with_txt {
    ($err_or_result_or_str:expr, $str:expr) => {{
        use error_stack::{Context, Report, Result, ResultExt};
        use $crate::_aer_inner;

        let foo = _aer_inner!($err_or_result_or_str);
        $crate::spez! {
            for x = foo;

            // Not lazy if already a report:
            match<T> Report<T>  -> Report<T> {
                x.attach_printable($str)
            }

            // Otherwise, lazy:
            match<T, E: Context> Result<T, E>  -> Result<T, E> {
                x.attach_printable($str)
            }
        }
    }};
}

/// A macro for building `AnyErr` reports easily from other error_stack errors, other reports and base errors outside of error_stack.
#[macro_export]
macro_rules! aer {
    () => {{
        use error_stack::Report;
        Report::new(AnyErr)
    }};

    ($err_or_result_or_str:expr) => {{
        use $crate::_aer_inner;
        _aer_inner!($err_or_result_or_str)
    }};

    ($err_or_result_or_str:expr, $str:expr) => {{
        use $crate::_aer_inner_with_txt;
        _aer_inner_with_txt!($err_or_result_or_str, $str)
    }};

    ($err_or_result_or_str:expr, $str:expr, $($arg:expr),*) => {{
        use $crate::_aer_inner_with_txt;
        _aer_inner_with_txt!($err_or_result_or_str, format!($str, $($arg),*))
    }};
}

/// A macro for creating a TracedErr from a string or another existing error.
#[macro_export]
macro_rules! err {

    ($str_or_err:expr) => {{
        use $crate::errors::TracedErr;
        use std::error::Error;

        $crate::spez! {
            for x = $str_or_err;
            // If its an error, directly convert to TracedError
            match<T: Error + Send + 'static> T  -> TracedErr{
                TracedErr::from(x)
            }
            // Otherwise if a string, or something else that can be converted to a string, pass in separate fn:
            match<T: Into<String>> T -> TracedErr {
                TracedErr::from_str(x)
            }
        }
    }};

    ($str:expr, $($arg:expr),*) => {{
        use $crate::errors::TracedErr;

        TracedErr::from_str(format!($str, $($arg),*))
    }};
}

/// When working in a function that cannot return a result, wrap a block in this macro to panic with the formatted error if it errors.
#[macro_export]
macro_rules! panic_on_err {
    ($closure:block) => {{
        use $crate::errors::TracedErr;

        match (|| -> Result<_, TracedErr> { $closure })() {
            Ok(s) => s,
            Err(e) => {
                panic!("{}", e);
            }
        }
    }};
}
