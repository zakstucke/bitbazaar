mod any;
mod macros;

pub use any::AnyErr;

pub(crate) mod prelude {
    #[allow(unused_imports)]
    pub use error_stack::{bail, report, Result, ResultExt};

    #[allow(unused_imports)]
    pub use super::any::AnyErr;
    #[allow(unused_imports)]
    pub use crate::anyerr;
}
