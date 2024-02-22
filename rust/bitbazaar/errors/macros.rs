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

/// A macro for building `Report<ArbitraryErrorStackErr>` objects with string context easily.
///
/// `err!(Err)` is equivalent to `Report::new(Err)`
///
/// `err!(Err, "foo")` is equivalent to `Report::new(Err).attach_printable("foo")`
///
/// `err!(Err, "foo: {}", "bar")` is equivalent to `Report::new(Err).attach_printable(format!("foo: {}", "bar"))`///
#[macro_export]
macro_rules! err {
    ($err_variant:expr) => {{
        use error_stack::Report;

        Report::new($err_variant)
    }};

    ($err_variant:expr, $str:expr) => {{
        use error_stack::Report;

        Report::new($err_variant).attach_printable($str)
    }};

    ($err_variant:expr, $str:expr, $($arg:expr),*) => {{
        use error_stack::Report;

        Report::new($err_variant).attach_printable(format!($str, $($arg),*))
    }};
}

/// When working in a function that cannot return a result, use this to auto panic with the formatted error if something goes wrong.
///
/// Allows use of e.g. `?` in the block.
#[macro_export]
macro_rules! panic_on_err {
    ($content:block) => {{
        #[allow(clippy::redundant_closure_call)]
        match ((|| $content)()) {
            Ok(s) => s,
            Err(e) => {
                panic!("{:?}", e);
            }
        }
    }};
}

/// When working in a function that cannot return a result, use this to auto panic with the formatted error if something goes wrong.
///
/// Allows use of e.g. `?` in the block.
#[macro_export]
macro_rules! panic_on_err_async {
    ($content:block) => {{
        match (async { $content }).await {
            Ok(s) => s,
            Err(e) => {
                panic!("{:?}", e);
            }
        }
    }};
}

#[cfg(test)]
mod tests {
    use futures::FutureExt;
    use rstest::*;

    use crate::prelude::*;

    #[rstest]
    fn panic_on_err() {
        // Should work fine:
        let result = panic_on_err!({
            // ? syntax should work, test with something fallible:
            let _ = Ok::<_, AnyErr>(1)?;
            Ok::<_, AnyErr>(1)
        });
        assert_eq!(result, 1);

        let should_err = std::panic::catch_unwind(|| {
            panic_on_err!({
                // ? syntax should work, test with something fallible:
                let _ = Ok::<_, AnyErr>(1)?;
                Err(anyerr!("foo"))
            });
        });
        assert!(should_err.is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn panic_on_err_async() {
        let async_result = panic_on_err_async!({
            // ? syntax should work, test with something fallible:
            let _ = Ok::<_, AnyErr>(1)?;

            tokio::time::sleep(std::time::Duration::from_nanos(1)).await;
            Ok::<_, AnyErr>(1)
        });
        assert_eq!(async_result, 1);

        let should_err = async {
            panic_on_err_async!({
                // ? syntax should work, test with something fallible:
                let _ = Ok::<_, AnyErr>(1)?;

                futures::future::ready(1).await;
                Err(anyerr!("foo"))
            });
        };

        assert!(should_err.catch_unwind().await.is_err());
    }
}
