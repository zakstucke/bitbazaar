#![allow(missing_docs)]
// Above needed because rstest fixture macro seems to produce undocumented functions and structs.

use rstest::*;
use tracing::Level;

use crate::log::GlobalLog;
use crate::prelude::*;

/// Include this in a test to turn on logging globally.
#[fixture]
#[once]
pub fn logging(#[default(Level::TRACE)] level: Level) {
    panic_on_err!({
        GlobalLog::setup_quick_stdout_global_logging(level)?;
        Ok::<(), error_stack::Report<AnyErr>>(())
    })
}
