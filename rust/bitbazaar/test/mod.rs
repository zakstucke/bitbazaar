/// Useful rstest fixtures.
pub mod fixtures;

/// Useful test utilities.
pub mod test_utils;

/// Default modules to bring into scope within test modules.
pub mod prelude {
    pub use rstest::*;

    pub use crate::prelude::*;
    pub use crate::test::fixtures::*;
    pub use crate::test::test_utils::*;
}
