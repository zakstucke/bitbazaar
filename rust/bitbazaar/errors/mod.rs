mod any;
mod macros;

pub use any::AnyErr;

/// Shorthand for a [`Result`] with a [`Report`] as the error variant
pub type RResult<T, C> = Result<T, error_stack::Report<C>>;

pub(crate) mod prelude {
    #[allow(unused_imports)]
    pub use error_stack::{Report, ResultExt};

    #[allow(unused_imports)]
    pub use super::{AnyErr, RResult};

    #[allow(unused_imports)]
    pub use crate::{anyerr, err, panic_on_err, panic_on_err_async};
}
