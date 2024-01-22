mod any;
mod macros;

pub use any::AnyErr;

pub(crate) mod prelude {
    pub use error_stack::{bail, report, Result, ResultExt};

    pub use super::any::AnyErr;
    pub use crate::anyerr;
}
