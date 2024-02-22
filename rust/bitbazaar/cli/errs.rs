use super::CmdOut;

/// User facing error type for Bash functionality.
#[derive(Debug)]
pub enum BashErr {
    /// BashSyntaxError
    BashSyntaxError(CmdOut),

    /// BashFeatureUnsupported
    BashFeatureUnsupported(CmdOut),

    /// InternalError
    InternalError(CmdOut),
}

impl BashErr {
    /// Get the CmdOut from the error.
    pub fn cmd_out(&self) -> &CmdOut {
        match self {
            BashErr::BashSyntaxError(cmd_out) => cmd_out,
            BashErr::BashFeatureUnsupported(cmd_out) => cmd_out,
            BashErr::InternalError(cmd_out) => cmd_out,
        }
    }
}

impl std::fmt::Display for BashErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BashErr::BashSyntaxError(cmd_out) => write!(f, "BashSyntaxError: couldn't parse bash script.\n{}", cmd_out.fmt_attempted_commands()),
            BashErr::BashFeatureUnsupported(cmd_out) => {
                write!(f, "BashFeatureUnsupported: feature in script is valid bash, but unsupported.\n{}", cmd_out.fmt_attempted_commands())
            }
            BashErr::InternalError(cmd_out) => write!(
                f,
                "InternalError: this shouldn't occur, open an issue at https://github.com/zakstucke/bitbazaar/issues\n{}", cmd_out.fmt_attempted_commands()
            ),
        }
    }
}

impl error_stack::Context for BashErr {}

/// Internal shell errors. Some of which should be handled.
#[derive(Debug, strum::Display)]
pub enum ShellErr {
    BashFeatureUnsupported,
    BashSyntaxError,
    Exit,
    InternalError,
}

impl error_stack::Context for ShellErr {}

/// Internal shell errors. Some of which should be handled.
#[derive(Debug, strum::Display)]
pub enum BuiltinErr {
    Exit,
    Unsupported,
    #[allow(dead_code)]
    InternalError,
}

impl error_stack::Context for BuiltinErr {}
