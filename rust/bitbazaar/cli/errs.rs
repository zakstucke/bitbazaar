use super::BashOut;

/// User facing error type for Bash functionality.
#[derive(Debug)]
pub enum BashErr {
    /// BashSyntaxError
    BashSyntaxError(BashOut),

    /// BashFeatureUnsupported
    BashFeatureUnsupported(BashOut),

    /// InternalError
    InternalError(BashOut),
}

impl BashErr {
    /// Get the BashOut from the error.
    pub fn bash_out(&self) -> &BashOut {
        match self {
            BashErr::BashSyntaxError(bash_out) => bash_out,
            BashErr::BashFeatureUnsupported(bash_out) => bash_out,
            BashErr::InternalError(bash_out) => bash_out,
        }
    }
}

impl std::fmt::Display for BashErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BashErr::BashSyntaxError(bash_out) => write!(f, "BashSyntaxError: couldn't parse bash script.\n{}", bash_out.fmt_attempted_commands()),
            BashErr::BashFeatureUnsupported(bash_out) => {
                write!(f, "BashFeatureUnsupported: feature in script is valid bash, but unsupported.\n{}", bash_out.fmt_attempted_commands())
            }
            BashErr::InternalError(bash_out) => write!(
                f,
                "InternalError: this shouldn't occur, open an issue at https://github.com/zakstucke/bitbazaar/issues\n{}", bash_out.fmt_attempted_commands()
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
