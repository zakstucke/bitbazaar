use error_stack::Report;

/// Further extensions on top of [`error_stack::ResultExt`].
pub trait BitbazaarResultExt {
    /// The [`Context`] type of the [`Result`].
    type Context: error_stack::Context;

    /// Type of the [`Ok`] value in the [`Result`]
    type Ok;

    /// Attach the current location to the stacktrace of the [`Report`] inside the [`Result`] if it's in error state.
    /// This is useful in 2 cases:
    /// - Async functions, therefore can't use #[track_caller] yet, once stable those uses can be removed. See <https://github.com/rust-lang/rust/issues/110011>.
    /// - When the same error is propagating through a middleman fn, and we really want to know the middleman specific location.
    #[track_caller]
    fn loc(self) -> Result<Self::Ok, Report<Self::Context>>;
}

impl<T, C: error_stack::Context> BitbazaarResultExt for Result<T, Report<C>> {
    type Context = C;
    type Ok = T;

    #[track_caller]
    fn loc(self) -> Result<T, Report<C>> {
        match self {
            Ok(ok) => Ok(ok),
            Err(report) => {
                Err(report.attach_printable(format!("at {}", std::panic::Location::caller())))
            }
        }
    }
}
