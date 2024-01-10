use crate::errors::TracedResult;

/// The result of running a command
pub struct CmdOut {
    /// The stdout of the command:
    pub stdout: String,
    /// The stderr of the command:
    pub stderr: String,
    /// The exit code of the command:
    pub code: i32,
}

impl CmdOut {
    /// Returns true when the command exited with a zero exit code.
    pub fn success(&self) -> bool {
        self.code == 0
    }

    /// Combines the stdout and stderr into a single string.
    pub fn std_all(&self) -> String {
        if !self.stdout.is_empty() && !self.stderr.is_empty() {
            format!("{}\n{}", self.stdout, self.stderr)
        } else if !self.stdout.is_empty() {
            self.stdout.clone()
        } else {
            self.stderr.clone()
        }
    }
}

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
pub fn run_cmd<S: Into<String>>(cmd_str: S) -> TracedResult<CmdOut> {
    let (code, output, error) = run_script::run(
        cmd_str.into().as_str(),
        &vec![],
        &run_script::ScriptOptions::new(),
    )?;

    Ok(CmdOut {
        stdout: output,
        stderr: error,
        code,
    })
}
