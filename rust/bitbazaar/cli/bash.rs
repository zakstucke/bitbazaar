use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use super::{errs::ShellErr, shell::Shell, BashErr, BashOut};
use crate::prelude::*;

/// Execute an arbitrary bash script.
///
/// WARNING: this opens up the possibility of dependency injection attacks, so should only be used when the command is trusted.
/// If compiled usage is all that's needed, use something like rust_cmd_lib instead, which only provides a macro literal interface.
/// <https://github.com/rust-shell-script/rust_cmd_lib>
///
/// This is a pure rust implementation and doesn't rely on bash being available to make it compatible with windows.
/// Given that, it only implements a subset of bash features, and is not intended to be a full bash implementation.
///
/// Purposeful deviations from bash:
/// - set -e is enabled by default, each cmd line will stop if it fails
///
/// Assume everything is unimplemented unless stated below:
/// - `&&` and
/// - `||` or
/// - `!` exit code negation
/// - `|` pipe
/// - `~` home dir
/// - `foo=bar` param setting
/// - `$foo` param substitution
/// - `$(echo foo)` command substitution
/// - `'` quotes
/// - `"` double quotes
/// - `\` escaping
/// - `(...)` simple compound commands e.g. (echo foo && echo bar)
/// - Basic file/stderr/stdout redirection
///
/// This should theoretically work with multi line full bash scripts but only tested with single line commands.
pub struct Bash {
    // The commands that will be loaded in to run, treated as && separated (only running the next if the last succeeded):
    cmds: Vec<String>,
    // Optional override of the root dir to run the commands in:
    root_dir: Option<PathBuf>,
    // Extra environment variables to run the commands with:
    env_vars: HashMap<String, String>,
}

impl Default for Bash {
    fn default() -> Self {
        Self::new()
    }
}

impl Bash {
    /// Create a new [`Bash`] builder.
    pub fn new() -> Self {
        Self {
            cmds: Vec::new(),
            root_dir: None,
            env_vars: HashMap::new(),
        }
    }

    /// Add a new piece of logic to the bash script. E.g. a line of bash.
    ///
    /// Multiple commands added to a [`Bash`] instance will be treated as newline separated.
    pub fn cmd(self, cmd: impl Into<String>) -> Self {
        let mut cmds = self.cmds;
        cmds.push(cmd.into());
        Self {
            cmds,
            root_dir: self.root_dir,
            env_vars: self.env_vars,
        }
    }

    /// Set the root directory to run the commands in.
    ///
    /// By default, the current process's root directory is used.
    pub fn chdir(self, root_dir: &Path) -> Self {
        Self {
            cmds: self.cmds,
            root_dir: Some(root_dir.to_path_buf()),
            env_vars: self.env_vars,
        }
    }

    /// Add an environment variable to the bash script.
    pub fn env(self, name: impl Into<String>, val: impl Into<String>) -> Self {
        let mut env_vars = self.env_vars;
        env_vars.insert(name.into(), val.into());
        Self {
            cmds: self.cmds,
            root_dir: self.root_dir,
            env_vars,
        }
    }

    /// Execute the current contents of the bash script.
    pub fn run(self) -> Result<BashOut, BashErr> {
        if self.cmds.is_empty() {
            return Ok(BashOut::empty());
        }

        let mut shell = Shell::new(self.env_vars, self.root_dir)
            .map_err(|e| shell_to_bash_err(BashOut::empty(), e))?;

        if let Err(e) = shell.execute_command_strings(self.cmds) {
            return Err(shell_to_bash_err(shell.into(), e));
        }

        Ok(shell.into())
    }
}

fn shell_to_bash_err(
    mut bash_out: BashOut,
    e: error_stack::Report<ShellErr>,
) -> error_stack::Report<BashErr> {
    // Doesn't really make sense, but set the exit code to 1 if 0, as technically the command errored even though it was the runner itself that errored and the command might not have been attempted.
    if bash_out.code() == 0 {
        bash_out.override_code(1);
    }
    match e.current_context() {
        ShellErr::Exit => e.change_context(BashErr::InternalError(bash_out)).attach_printable(
            "Shouldn't occur, shell exit errors should have been managed internally, not an external error.",
        ),
        ShellErr::InternalError => e.change_context(BashErr::InternalError(bash_out)),
        ShellErr::BashFeatureUnsupported => e.change_context(BashErr::BashFeatureUnsupported(bash_out)),
        ShellErr::BashSyntaxError => e.change_context(BashErr::BashSyntaxError(bash_out)),
    }
}
