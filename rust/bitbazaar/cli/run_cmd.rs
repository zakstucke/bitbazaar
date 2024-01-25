use super::CmdOut;
use crate::{cli::bash, errors::prelude::*};

#[derive(Debug, strum::Display)]
pub enum CmdErr {
    #[strum(serialize = "CmdErr (BashFeatureUnsupported): Bash feature in script unsupported.")]
    BashFeatureUnsupported,
    #[strum(serialize = "CmdErr (BashSyntaxError): Bash syntax error.")]
    BashSyntaxError,
    #[strum(serialize = "CmdErr (BashUTF8Error): Could not decode UTF-8 from cmd.")]
    BashUTF8Error,
    #[strum(
        serialize = "CmdErr (NoHomeDirectory): A tilde (~) is used in a script, can't find home directory."
    )]
    NoHomeDirectory,
    #[strum(serialize = "CmdErr (Internal): most like something wrong internally.")]
    InternalError,
}

impl error_stack::Context for CmdErr {}

/// Run a dynamic shell command and return the output.
///
/// WARNING: this opens up the possibility of dependency injection attacks, so should only be used when the command is trusted.
/// If compiled usage is all that's needed, use something like xshell instead, which only provides a macro literal interface.
///
/// This doesn't work with command line substitution (e.g. `$(echo foo)`), but is tested to work with:
/// - `&&` and
/// - `||` or
/// - `|` pipe
/// - `~` home dir
pub fn run_cmd<S: Into<String>>(cmd_str: S) -> Result<CmdOut, CmdErr> {
    let cmd_str = cmd_str.into();

    bash::execute_bash(cmd_str)
}
