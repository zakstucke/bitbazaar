/// Error type for `execute_bash()`.
#[derive(Debug, strum::Display)]
pub enum CmdErr {
    /// Bash feature in script unsupported.
    #[strum(serialize = "CmdErr (BashFeatureUnsupported): Bash feature in script unsupported.")]
    BashFeatureUnsupported,
    /// Bash syntax error.
    #[strum(serialize = "CmdErr (BashSyntaxError): Bash syntax error.")]
    BashSyntaxError,
    /// Could not decode UTF-8 from cmd.
    #[strum(serialize = "CmdErr (BashUTF8Error): Could not decode UTF-8 from cmd.")]
    BashUTF8Error,
    /// A tilde (~) is used in a script, can't find home directory.
    #[strum(
        serialize = "CmdErr (NoHomeDirectory): A tilde (~) is used in a script, can't find home directory."
    )]
    NoHomeDirectory,
    /// Most like something wrong internally.
    #[strum(serialize = "CmdErr (Internal): most like something wro ng internally.")]
    InternalError,
}

impl error_stack::Context for CmdErr {}
