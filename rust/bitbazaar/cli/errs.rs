/// User facing error type for Bash functionality.
#[derive(Debug, strum::Display)]
pub enum BashErr {
    /// BashSyntaxError
    #[strum(serialize = "BashSyntaxError: couldn't parse bash script.")]
    BashSyntaxError,

    /// BashFeatureUnsupported
    #[strum(
        serialize = "BashFeatureUnsupported: feature in script is valid bash, but unsupported."
    )]
    BashFeatureUnsupported,

    /// InternalError
    #[strum(
        serialize = "InternalError: this shouldn't occur, open an issue at https://github.com/zakstucke/bitbazaar/issues"
    )]
    InternalError,
}

impl error_stack::Context for BashErr {}

/// Internal shell errors. Some of which should be handled.
#[derive(Debug, strum::Display)]
pub enum ShellErr {
    BashFeatureUnsupported,
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
