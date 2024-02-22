use std::{
    io::Write,
    process::{self, Stdio},
    str,
};

use conch_parser::ast;

use super::{
    builtins::Builtin,
    errs::{BuiltinErr, ShellErr},
    redirect::handle_redirect,
    shell::Shell,
    BashOut,
};
use crate::prelude::*;

#[derive(Debug)]
pub enum VariCommand {
    /// A builtin command implemented directly in rust, alongside the arguments to pass.
    Builtin(String, Builtin, Vec<String>),
    Normal(process::Command),
    // Instead of running a command, use the given string as stdin for the next command, or use as stdout if final.
    PipedStdout(String),
    Redirect(ast::DefaultRedirect),
}

#[derive(Default)]
pub struct PipeRunner {
    pub negate: bool,
    commands: Vec<VariCommand>,
    // These are the individual outputs of the commands, in various formats, previous will be modified/partially consumed depending on later commands.
    outputs: Vec<RunnerBashOut>,
}

pub enum RunnerBashOut {
    Concrete(ConcreteOutput),
    Pending(process::Child),
}

impl Default for RunnerBashOut {
    fn default() -> Self {
        RunnerBashOut::Concrete(ConcreteOutput::default())
    }
}

#[derive(Default)]
pub struct ConcreteOutput {
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub code: Option<i32>,
}

impl RunnerBashOut {
    fn into_shell(self, shell: &mut Shell) -> Result<(), ShellErr> {
        match self {
            RunnerBashOut::Concrete(conc) => {
                if let Some(stdout) = conc.stdout {
                    shell.push_stdout(&stdout);
                }

                if let Some(stderr) = conc.stderr {
                    shell.push_stderr(&stderr);
                }

                if let Some(code) = conc.code {
                    shell.set_code(code);
                }
            }
            // This is probably the last command:
            RunnerBashOut::Pending(child) => {
                let output = child
                    .wait_with_output()
                    .change_context(ShellErr::InternalError)?;

                shell.push_stdout(
                    str::from_utf8(&output.stdout).change_context(ShellErr::InternalError)?,
                );
                shell.push_stderr(
                    str::from_utf8(&output.stderr).change_context(ShellErr::InternalError)?,
                );
                shell.set_code(output.status.code().unwrap_or(1));
            }
        }

        Ok(())
    }
}

impl From<BashOut> for RunnerBashOut {
    fn from(bash_out: BashOut) -> Self {
        RunnerBashOut::Concrete(ConcreteOutput {
            stdout: Some(bash_out.stdout()),
            stderr: Some(bash_out.stderr()),
            code: Some(bash_out.code()),
        })
    }
}

impl PipeRunner {
    /// Add a new command to the runner.
    pub fn add(&mut self, args: Vec<String>) -> Result<(), ShellErr> {
        let first_arg = args
            .first()
            .ok_or_else(|| err!(ShellErr::InternalError, "No command provided"))?
            .to_string();

        // Either use a rust builtin if implemented, or delegate to the OS:
        let vari = if let Some(builtin) = super::builtins::BUILTINS.get(first_arg.as_str()) {
            VariCommand::Builtin(
                first_arg,
                *builtin,
                // Remaining args:
                args.into_iter().skip(1).collect(),
            )
        } else {
            let mut cmd = process::Command::new(first_arg);
            if args.len() > 1 {
                cmd.args(args.into_iter().skip(1));
            }
            VariCommand::Normal(cmd)
        };
        self.commands.push(vari);

        Ok(())
    }

    pub fn add_redirect(&mut self, redirect: &ast::DefaultRedirect) -> Result<(), ShellErr> {
        self.commands.push(VariCommand::Redirect(redirect.clone()));
        Ok(())
    }

    pub fn add_piped_stdout(&mut self, stdout: String) {
        self.commands.push(VariCommand::PipedStdout(stdout));
    }

    pub fn run(mut self, shell: &mut Shell) -> Result<(), ShellErr> {
        for command in self.commands.into_iter() {
            let last_out = self.outputs.last_mut();
            let next_out: RunnerBashOut = match command {
                VariCommand::Redirect(redirect) => handle_redirect(shell, last_out, redirect)?,
                VariCommand::Builtin(name, builtin, args) => match builtin(shell, &args) {
                    Ok(bash_out) => bash_out.into(),
                    Err(mut e) => {
                        e = e.attach_printable(format!("Command: '{}' args: '{:?}'", name, args));
                        match e.current_context() {
                            BuiltinErr::Exit => return Err(e.change_context(ShellErr::Exit)),
                            BuiltinErr::Unsupported => {
                                return Err(e.change_context(ShellErr::BashFeatureUnsupported))
                            }
                            BuiltinErr::InternalError => {
                                return Err(e.change_context(ShellErr::InternalError))
                            }
                        }
                    }
                },
                VariCommand::PipedStdout(stdout) => RunnerBashOut::Concrete(ConcreteOutput {
                    stdout: Some(stdout),
                    stderr: None,
                    code: None,
                }),
                VariCommand::Normal(mut command) => {
                    // Set the working dir:
                    command.current_dir(shell.active_dir()?);

                    // Add all the shell args to the env of the command:
                    command.envs(shell.vars.clone());

                    // Pipe in stdin if needed:
                    let mut str_stdin = None;
                    if let Some(last_out) = last_out {
                        match last_out {
                            // Might contain stdout, in which case take it and use as stdin:
                            RunnerBashOut::Concrete(conc) => {
                                if let Some(stdout) = conc.stdout.take() {
                                    str_stdin = Some(stdout);
                                    command.stdin(Stdio::piped());
                                }
                            }

                            // Child process, pipe its handle through to the next command, keeping track of the stderr:
                            RunnerBashOut::Pending(child) => {
                                if let Some(stdout) = child.stdout.take() {
                                    command.stdin(stdout);
                                }
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
                                    err!(ShellErr::InternalError, "Couldn't access stdin handle.")
                                })?;

                                stdin_handle
                                    .write_all(s.as_bytes())
                                    .change_context(ShellErr::InternalError)?;
                            }

                            RunnerBashOut::Pending(child)
                        }
                        Err(e) => {
                            // Command might error straight away, in which case convert the err to stderr.
                            // this gives more or less parity with bash:
                            RunnerBashOut::Concrete(ConcreteOutput {
                                // If the spawn errored, something went wrong, so set the code:
                                code: Some(e.raw_os_error().unwrap_or(1)),
                                stdout: None,
                                stderr: {
                                    let mut err_out = e.to_string();
                                    if !err_out.trim().is_empty() {
                                        if !err_out.ends_with('\n') {
                                            err_out.push('\n');
                                        }
                                        Some(err_out)
                                    } else {
                                        None
                                    }
                                },
                            })
                        }
                    }
                }
            };

            self.outputs.push(next_out);
        }

        // Load all the outputs into the shell:
        for output in self.outputs {
            output.into_shell(shell)?;
        }

        // Negate the code if needed:
        if self.negate {
            shell.set_code(if shell.code() == 0 { 1 } else { 0 });
        }

        Ok(())
    }
}
