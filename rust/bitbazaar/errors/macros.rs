#[macro_export]
/// A macro for creating a TracedErr from a string or another existing error.
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
