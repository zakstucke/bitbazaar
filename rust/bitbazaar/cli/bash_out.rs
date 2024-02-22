use crate::prelude::*;

/// The result of an individual command.
#[derive(Debug, Clone)]
pub struct CmdResult {
    /// The command that was run
    pub command: String,
    /// The exit code of the command
    pub code: i32,
    /// The stdout of the command
    pub stdout: String,
    /// The stderr of the command
    pub stderr: String,
}

impl CmdResult {
    /// Create a new CmdResult.
    pub(crate) fn new(
        command: impl Into<String>,
        code: i32,
        stdout: impl Into<String>,
        stderr: impl Into<String>,
    ) -> Self {
        Self {
            command: command.into(),
            code,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }
}

/// The result of running a command
#[derive(Debug, Clone)]
pub struct BashOut {
    /// All commands that were run, if a command fails, it will be the last command in this vec, the remaining were not attempted.
    pub command_results: Vec<CmdResult>,

    code_override: Option<i32>,
}

impl From<CmdResult> for BashOut {
    fn from(result: CmdResult) -> Self {
        Self {
            command_results: vec![result],
            code_override: None,
        }
    }
}

/// Public interface
impl BashOut {
    /// Returns the exit code of the last command that was run.
    pub fn code(&self) -> i32 {
        if let Some(code) = self.code_override {
            code
        } else {
            self.command_results.last().map(|r| r.code).unwrap_or(0)
        }
    }

    /// Returns true when the command exited with a zero exit code.
    pub fn success(&self) -> bool {
        self.code() == 0
    }

    /// Combines the stdout from each run command into a single string.
    pub fn stdout(&self) -> String {
        let mut out = String::new();
        for result in &self.command_results {
            out.push_str(&result.stdout);
        }
        out
    }

    /// Combines the stderr from each run command into a single string.
    pub fn stderr(&self) -> String {
        let mut out = String::new();
        for result in &self.command_results {
            out.push_str(&result.stderr);
        }
        out
    }

    /// Combines the stdout AND stderr from each run command into a single string.
    pub fn std_all(&self) -> String {
        let mut out = String::new();
        for result in &self.command_results {
            out.push_str(&result.stdout);
            out.push_str(&result.stderr);
        }
        out
    }

    /// Returns the stdout from the final command that was run.
    pub fn last_stdout(&self) -> String {
        self.command_results
            .last()
            .map(|r| r.stdout.clone())
            .unwrap_or_default()
    }

    /// Returns the stderr from the final command that was run.
    pub fn last_stderr(&self) -> String {
        self.command_results
            .last()
            .map(|r| r.stderr.clone())
            .unwrap_or_default()
    }

    /// Returns the stdout AND stderr from the final command that was run.
    pub fn last_std_all(&self) -> String {
        let mut out = String::new();
        out.push_str(&self.last_stdout());
        out.push_str(&self.last_stderr());
        out
    }

    /// Pretty format the attempted commands, with the exit code included on the final line.
    pub fn fmt_attempted_commands(&self) -> String {
        if !self.command_results.is_empty() {
            let mut out = "Attempted commands:\n".to_string();
            for (index, result) in self.command_results.iter().enumerate() {
                // Indent the commands by a bit of whitespace:
                out.push_str("   ");
                // Add cmd number:
                out.push_str(format!("{}. ", index).as_str());
                out.push_str(result.command.trim());
                // Newline if not last:
                if index < self.command_results.len() - 1 {
                    out.push('\n');
                }
            }
            // On the last line, add <-- exited with code: X
            out.push_str(&format!(" <-- exited with code: {}", self.code()));
            out
        } else {
            "No commands run!".to_string()
        }
    }

    /// Throw an error if the last command run was not successful.
    pub fn throw_on_bad_code<T: error_stack::Context>(&self, err_variant: T) -> Result<(), T> {
        if self.success() {
            Ok(())
        } else {
            Err(err!(
                err_variant,
                "Cli var command returned a non zero exit code: {}. Std output: {}",
                self.code(),
                self.std_all()
            )
            .attach_printable(self.fmt_attempted_commands()))
        }
    }
}

/// Private interface
impl BashOut {
    pub(crate) fn new(command_results: Vec<CmdResult>) -> Self {
        Self {
            command_results,
            code_override: None,
        }
    }

    /// Create a new BashOut.
    pub(crate) fn empty() -> Self {
        Self {
            command_results: Vec::new(),
            code_override: None,
        }
    }

    pub(crate) fn override_code(&mut self, code: i32) {
        self.code_override = Some(code);
    }
}
