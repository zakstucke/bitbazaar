use crate::log::GlobalLog;
use tracing::Level;

use crate::testing::prelude::*;

/// Include this in a test to turn on logging globally.
#[fixture]
#[once]
pub fn logging(#[default(Level::TRACE)] level: Level) {
    panic_on_err!({
        GlobalLog::setup_quick_stdout_global_logging(level)?;
        Ok::<(), error_stack::Report<AnyErr>>(())
    })
}
