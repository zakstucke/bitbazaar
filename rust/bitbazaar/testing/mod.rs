pub mod fixtures;

pub mod prelude {
    #[allow(unused_imports)]
    pub use rstest::*;

    #[allow(unused_imports)]
    pub use crate::prelude::*;

    #[allow(unused_imports)]
    pub use crate::testing::fixtures::*;
}
