#[allow(unused_imports)]
pub use bitbazaar::{anyerr, err, errors::AnyErr, panic_on_err, panic_on_err_async};
#[allow(unused_imports)]
pub use error_stack::{Report, Result, ResultExt};
#[allow(unused_imports)]
pub use tracing::{debug, error, info, warn};

/// Shorthand for a [`Result`] with a [`Report`] as the error variant
#[allow(dead_code)]
pub type RResult<T, C> = Result<T, Report<C>>;
