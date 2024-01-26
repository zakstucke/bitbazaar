use std::{
    ffi::OsStr,
    io::{Read, Write},
    process::{self, ChildStderr, Stdio},
    str,
};

use super::{bash::Shell, CmdErr};
use crate::prelude::*;

enum VariCommand {
    Normal(process::Command),
    // Instead of running a command, use the given string as stdin for the next command, or use as stdout if final.
    PipedStdout(String),
}

/// Allows passing in stdin from precomputed commands run by different runners.
enum VariStdin {
    Stdio(Stdio),
    String(String),
}

#[derive(Default)]
pub struct PipeRunner {
    pub negate: bool,
    commands: Vec<VariCommand>,
    stderrs: Vec<ChildStderr>,
}

impl PipeRunner {
    /// Add a new command to the runner.
    pub fn add<S>(&mut self, args: &[S]) -> Result<(), CmdErr>
    where
        S: AsRef<OsStr>,
    {
        let first_arg = args
            .iter()
            .next()
            .ok_or_else(|| err!(CmdErr::InternalError, "No command provided"))?;

        let mut cmd = process::Command::new(first_arg);
        if args.len() > 1 {
            cmd.args(args.iter().skip(1));
        }

        self.commands.push(VariCommand::Normal(cmd));

        Ok(())
    }

    pub fn add_piped_stdout(&mut self, stdout: String) {
        self.commands.push(VariCommand::PipedStdout(stdout));
    }

    pub fn run(mut self, shell: &mut Shell) -> Result<(), CmdErr> {
        let num_cmds = self.commands.len();

        let mut stdin: Option<VariStdin> = None;
        for (index, command) in self.commands.into_iter().enumerate() {
            let is_last = index == num_cmds - 1;

            match command {
                VariCommand::PipedStdout(stdout) => {
                    if !is_last {
                        // If not last, need to use this as the stdin to the next command:
                        stdin = Some(VariStdin::String(stdout));
                    } else {
                        // If last command, just use as output:
                        shell.stdout.push_str(&stdout);
                    }
                }
                VariCommand::Normal(mut command) => {
                    // Add all the shell args to the env of the command:
                    command.envs(shell.vars.clone());

                    // Pipe in stdin if needed:
                    let mut str_stdin = None;
                    if let Some(stdin) = stdin.take() {
                        match stdin {
                            VariStdin::Stdio(stdio) => {
                                command.stdin(stdio);
                            }
                            VariStdin::String(s) => {
                                str_stdin = Some(s);
                                command.stdin(Stdio::piped());
                            }
                        };
                    }

                    // Spawn the new command:
                    match command
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .spawn()
                    {
                        Ok(mut child) => {
                            // If needed, manually passing stdin from a string:
                            if let Some(s) = str_stdin {
                                let mut stdin_handle = child.stdin.take().ok_or_else(|| {
                                    err!(CmdErr::InternalError, "Couldn't access stdin handle.")
                                })?;

                                stdin_handle
                                    .write_all(s.as_bytes())
                                    .change_context(CmdErr::InternalError)?;
                            }

                            // Not last command, need to pipe into next:
                            if !is_last {
                                let stdout_handle = child.stdout.ok_or_else(|| {
                                    err!(
                                        CmdErr::InternalError,
                                        "No stdout handle from previous command."
                                    )
                                })?;

                                let stderr_handle = child.stderr.ok_or_else(|| {
                                    err!(
                                        CmdErr::InternalError,
                                        "No stderr handle from previous command."
                                    )
                                })?;

                                self.stderrs.push(stderr_handle);
                                stdin = Some(VariStdin::Stdio(stdout_handle.into()));
                            } else {
                                // Last command, need to finalise all:

                                // Wait for the output of the final command:
                                let output = child
                                    .wait_with_output()
                                    .change_context(CmdErr::InternalError)?;

                                // Load in the stderrs from previous commands:
                                for mut stderr in std::mem::take(&mut self.stderrs) {
                                    stderr
                                        .read_to_string(&mut shell.stderr)
                                        .change_context(CmdErr::InternalError)?;
                                }

                                // Add on the stderr from the final command:
                                shell.stderr.push_str(
                                    str::from_utf8(&output.stderr)
                                        .change_context(CmdErr::BashUTF8Error)?
                                        .to_string()
                                        .as_str(),
                                );

                                // Read the out from the final command:
                                shell.stdout.push_str(
                                    str::from_utf8(&output.stdout)
                                        .change_context(CmdErr::BashUTF8Error)?,
                                );

                                shell.code = output.status.code().unwrap_or(1);
                            }
                        }
                        Err(e) => {
                            // Command might error straight away, in which case convert the err to stderr.
                            // this gives more or less parity with bash:

                            let err_out = e.to_string();
                            if !err_out.trim().is_empty() {
                                shell.stderr.push_str(&err_out);
                                if !shell.stderr.ends_with('\n') {
                                    shell.stderr.push('\n');
                                }
                            }

                            // If the spawn errored, something went wrong, so set the code:
                            shell.code = e.raw_os_error().unwrap_or(1);
                        }
                    }
                }
            };
        }

        // Negate the code if needed:
        if self.negate {
            shell.code = if shell.code == 0 { 1 } else { 0 };
        }

        Ok(())
    }
}
