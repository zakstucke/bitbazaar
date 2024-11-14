use crate::prelude::*;

/// Wrap your main function in this to auto handle common program needs.
/// - Handles logging top-level errors and panics.
/// - Ensures all logging/tracing is flushed before exiting.
/// - Error exit codes, can still pass through downstream codes on happy path.
#[track_caller]
pub fn main_wrapper(
    main: impl FnOnce() -> RResult<std::process::ExitCode, AnyErr> + std::panic::UnwindSafe,
) -> std::process::ExitCode {
    use crate::log::record_exception;

    let exit_code_or_panic: Result<std::process::ExitCode, _> =
        std::panic::catch_unwind(|| match main() {
            Ok(downstream_exit_code) => downstream_exit_code,
            Err(err) => {
                record_exception("Exited with error.", format!("{:?}", err));
                1.into()
            }
        });

    let exit_code = match exit_code_or_panic {
        Ok(exit_code) => exit_code,
        Err(e) => {
            record_exception("Panicked.", format!("{:?}", e));
            1.into()
        }
    };

    // Try and make sure all telemetry has left the system before exiting, to prevent crucial error logs being lost:
    match crate::log::flush_and_consume() {
        Ok(_) => (),
        Err(err) => {
            // Should be an eprintln as something going wrong with logs, if we used logs it probably wouldn't make it out in time before exiting.
            eprintln!("Error flushing logs during cleanup: {:?}", err);
        }
    };

    exit_code
}
