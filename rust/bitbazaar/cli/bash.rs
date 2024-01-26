use std::{collections::HashMap, str};

use conch_parser::{ast, lexer::Lexer, parse::DefaultParser};

use super::{runner::PipeRunner, CmdErr, CmdOut};
use crate::prelude::*;

/// Execute an arbitrary bash script.
///
/// WARNING: this opens up the possibility of dependency injection attacks, so should only be used when the command is trusted.
/// If compiled usage is all that's needed, use something like rust_cmd_lib instead, which only provides a macro literal interface.
/// https://github.com/rust-shell-script/rust_cmd_lib
///
/// This is a pure rust implementation and doesn't rely on bash being available to make it compatible with windows.
/// Given that, it only implements a subset of bash features, and is not intended to be a full bash implementation.
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
///
/// This should theoretically work with multi line full bash scripts but only tested with single line commands.
pub fn execute_bash(cmd_str: &str) -> Result<CmdOut, CmdErr> {
    let cmd_str = cmd_str.trim();

    let lex = Lexer::new(cmd_str.chars());
    let parser = DefaultParser::new(lex);

    let top_cmds = parser
        .into_iter()
        .collect::<core::result::Result<Vec<_>, _>>()
        .change_context(CmdErr::BashSyntaxError)?;

    let mut shell = Shell::new();
    shell.run_top_cmds(top_cmds)?;

    Ok(shell.into())
}

#[derive(Debug)]
struct WordConcatState<'a> {
    active: usize,
    words: &'a Vec<ast::DefaultWord>,
}

pub struct Shell {
    /// Extra params/env vars added to this shell
    pub vars: HashMap<String, String>,
    pub code: i32,
    // Finalised output that won't be piped to another command and should be returned to the caller:
    pub stdout: String,
    pub stderr: String,
}

impl From<Shell> for CmdOut {
    fn from(val: Shell) -> Self {
        CmdOut {
            stdout: val.stdout,
            stderr: val.stderr,
            code: val.code,
        }
    }
}

impl Shell {
    fn new() -> Self {
        Self {
            vars: HashMap::new(),
            code: 0,
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    fn run_top_cmds(&mut self, cmds: Vec<ast::TopLevelCommand<String>>) -> Result<(), CmdErr> {
        // Each res equates to a line in a multi line bash script. E.g. a single line command will only have one res.
        for cmd in cmds {
            match cmd.0 {
                ast::Command::Job(job) => {
                    return Err(err!(
                        CmdErr::BashFeatureUnsupported,
                        "Jobs, i.e. asynchronous commands using '&' are not supported."
                    )
                    .attach_printable(format!("{job:?}")))
                }
                ast::Command::List(list) => {
                    // Run the first command in the chain:
                    self.run_listable_command(list.first)?;

                    // Run the remaining commands in the chain, breaking dependent on and/or with the last exit code:
                    for chain_cmd in list.rest.into_iter() {
                        match chain_cmd {
                            ast::AndOr::And(cmd) => {
                                // Only run if the last succeeded:
                                if self.code == 0 {
                                    self.run_listable_command(cmd)?;
                                }
                            }
                            ast::AndOr::Or(cmd) => {
                                // Only run if the last didn't succeed:
                                if self.code != 0 {
                                    self.run_listable_command(cmd)?;
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn run_listable_command(&mut self, cmd: ast::DefaultListableCommand) -> Result<(), CmdErr> {
        let mut pipe_runner = PipeRunner::default();

        match cmd {
            ast::ListableCommand::Single(cmd) => {
                debug!("Running single cmd: {:?}", cmd);
                self.add_pipe_command(&mut pipe_runner, &cmd)?;
            }
            ast::ListableCommand::Pipe(negate_code, cmds) => {
                // Mark whether the code should be negated or not:
                pipe_runner.negate = negate_code;

                for cmd in cmds {
                    self.add_pipe_command(&mut pipe_runner, &cmd)?;
                }
            }
        };

        pipe_runner.run(self)?;

        Ok(())
    }

    fn add_pipe_command(
        &mut self,
        pipe_runner: &mut PipeRunner,
        cmd: &ast::DefaultPipeableCommand,
    ) -> Result<(), CmdErr> {
        match cmd {
            ast::PipeableCommand::Simple(cmd) => self.add_simple_command(pipe_runner, cmd)?,
            ast::PipeableCommand::Compound(compound) => {
                // E.g. (echo foo && echo bar)
                match &compound.kind {
                    ast::CompoundCommandKind::Subshell(sub_cmds) => {
                        let mut shell = Shell::new();
                        shell.run_top_cmds(sub_cmds.clone())?;

                        // Add the stderr to the current shell:
                        self.stderr.push_str(&shell.stderr);

                        debug!("Compound cmd stdout: '{}'", shell.stdout);
                        // Add the pre-computed stdout to be used as stdin to the next command in the outer runner:
                        pipe_runner.add_piped_stdout(shell.stdout);
                    }
                    ast::CompoundCommandKind::Brace(_) => {
                        return Err(unsup(
                            "Compound brace. A group of commands that should be executed in the current environment.",
                        ));
                    }
                    ast::CompoundCommandKind::While(_) => {
                        return Err(unsup(
                            "Compound while. A command that executes its body as long as its guard exits successfully.",
                        ));
                    }
                    ast::CompoundCommandKind::Until(_) => {
                        return Err(unsup(
                            "Compound until. A command that executes its body as until as its guard exits unsuccessfully.",
                        ));
                    }
                    ast::CompoundCommandKind::If { .. } => {
                        return Err(unsup(
                            "Compound if. A conditional command that runs the respective command branch when a certain of the first condition that exits successfully.",
                        ));
                    }
                    ast::CompoundCommandKind::For { .. } => {
                        return Err(unsup(
                            "Compound for. A command that binds a variable to a number of provided words and runs its body once for each binding.",
                        ));
                    }
                    ast::CompoundCommandKind::Case { .. } => {
                        return Err(unsup(
                            "Compound case. A command that behaves much like a match statement in Rust, running a branch of commands if a specified word matches another literal or glob pattern.",
                        ));
                    }
                }
            }
            ast::PipeableCommand::FunctionDef(a, b) => Err(err!(
                CmdErr::BashFeatureUnsupported,
                "Functions not implemented."
            )
            .attach_printable(a.to_string())
            .attach_printable(format!("{b:?}")))?,
        };
        Ok(())
    }

    fn add_simple_command(
        &mut self,
        pipe_runner: &mut PipeRunner,
        cmd: &ast::DefaultSimpleCommand,
    ) -> Result<(), CmdErr> {
        // Get the environment variables the command (and all inner) need:
        let env = cmd
            .redirects_or_env_vars
            .iter()
            .map(|env_var| match env_var {
                ast::RedirectOrEnvVar::Redirect(_) => Err(err!(
                    CmdErr::BashFeatureUnsupported,
                    "Redirection not implemented."
                )
                .attach_printable(format!("{env_var:?}"))),
                ast::RedirectOrEnvVar::EnvVar(name, val) => {
                    let value = if let Some(val) = val {
                        self.process_complex_word(&val.0)?
                    } else {
                        "".to_string()
                    };
                    debug!("Setting env var: '{}'='{}'", name, value);
                    Ok((name, value))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut args = Vec::with_capacity(cmd.redirects_or_cmd_words.len());
        for arg in cmd.redirects_or_cmd_words.iter() {
            let arg_str = match arg {
                ast::RedirectOrCmdWord::Redirect(redirect) => {
                    return Err(err!(
                        CmdErr::BashFeatureUnsupported,
                        "Redirection not implemented."
                    )
                    .attach_printable(format!("{redirect:?}")))
                }
                ast::RedirectOrCmdWord::CmdWord(word) => self.process_complex_word(&word.0)?,
            };

            args.push(arg_str);
        }

        debug!("Final command args: {:?}", args);

        // Add the env vars to the current shell to in this command, later and parser expansions etc:
        for (name, val) in env.iter() {
            self.vars.insert(name.to_string(), val.to_string());
        }

        // Add only if has args, e.g. this command was "bar=3;" then no os command is actually needed:
        if !args.is_empty() {
            pipe_runner.add(&args)?;
        };

        Ok(())
    }

    fn process_complex_word(&mut self, word: &ast::DefaultComplexWord) -> Result<String, CmdErr> {
        match word {
            ast::ComplexWord::Single(word) => self.process_word(word, None, false),
            ast::ComplexWord::Concat(words) => {
                // Need to do some lookarounds, keep track of the active part of the complex word:
                let mut concat_state = WordConcatState { active: 0, words };
                let result = words
                    .iter()
                    .enumerate()
                    .map(|(index, word)| {
                        concat_state.active = index;
                        self.process_word(word, Some(&concat_state), false)
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join("");
                Ok(result)
            }
        }
    }

    fn process_word(
        &mut self,
        word: &ast::DefaultWord,
        concat_state: Option<&'_ WordConcatState<'_>>,
        is_lookaround: bool,
    ) -> Result<String, CmdErr> {
        Ok(match word {
            // Single quoted means no processing inside needed:
            ast::Word::SingleQuoted(word) => word.to_string(),
            ast::Word::Simple(word) => {
                self.process_simple_word(word, concat_state, is_lookaround)?
            }
            ast::Word::DoubleQuoted(words) => words
                .iter()
                .map(|word| self.process_simple_word(word, concat_state, is_lookaround))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .collect::<Vec<_>>()
                .join(""),
        })
    }

    fn process_simple_word(
        &mut self,
        word: &ast::DefaultSimpleWord,
        concat_state: Option<&'_ WordConcatState<'_>>,
        is_lookaround: bool,
    ) -> Result<String, CmdErr> {
        Ok(match word {
            ast::SimpleWord::Literal(lit) => lit.to_string(),
            ast::SimpleWord::Escaped(a) => a.to_string(),
            ast::SimpleWord::Tilde => {
                if self.expand_tilde(concat_state, is_lookaround)? {
                    // Convert to the user's home directory:
                    let home_dir =
                        homedir::get_my_home().change_context(CmdErr::NoHomeDirectory)?;
                    if let Some(home_dir) = home_dir {
                        home_dir.to_string_lossy().to_string()
                    } else {
                        return Err(err!(CmdErr::NoHomeDirectory));
                    }
                } else {
                    "~".to_string()
                }
            }
            ast::SimpleWord::Param(param) => self.process_param(param)?,
            ast::SimpleWord::Subst(sub) => self.process_substitution(sub)?,
            ast::SimpleWord::Colon => {
                return Err(unsup("':', useful for handling tilde expansions."));
            }
            ast::SimpleWord::Question => {
                return Err(unsup("'?', useful for handling pattern expansions."));
            }
            ast::SimpleWord::Star => {
                return Err(unsup("'*', useful for handling pattern expansions."));
            }
            ast::SimpleWord::SquareOpen => {
                return Err(unsup("'[', useful for handling pattern expansions."));
            }
            ast::SimpleWord::SquareClose => {
                return Err(unsup("']', useful for handling pattern expansions."));
            }
        })
    }

    fn process_param(&mut self, param: &ast::DefaultParameter) -> Result<String, CmdErr> {
        Ok(match param {
            ast::Parameter::Var(var) => {
                // First try variables in current shell, otherwise try env:
                let value = if let Some(val) = self.vars.get(var) {
                    val.clone()
                } else {
                    // Return the env var, or empty string if not set:
                    std::env::var(var).unwrap_or_else(|_| "".to_string())
                };
                debug!("Substituting param: '{}'='{}'", var, value);
                value
            }
            ast::Parameter::Positional(_) => {
                return Err(unsup("positional, e.g. '$0, $1, ..., $9, ${100}'."));
            }
            ast::Parameter::At => {
                return Err(unsup("$@'."));
            }
            ast::Parameter::Star => {
                return Err(unsup("'$*'."));
            }
            ast::Parameter::Pound => {
                return Err(unsup("'$#'."));
            }
            ast::Parameter::Question => {
                return Err(unsup("'$?'."));
            }
            ast::Parameter::Dash => {
                return Err(unsup("'$-'."));
            }
            ast::Parameter::Dollar => {
                return Err(unsup("'$$'."));
            }
            ast::Parameter::Bang => {
                return Err(unsup("'$!'."));
            }
        })
    }

    fn process_substitution(
        &mut self,
        sub: &ast::DefaultParameterSubstitution,
    ) -> Result<String, CmdErr> {
        match sub {
            ast::ParameterSubstitution::Command(cmds) => {
                // Run the nested command, from my tests with terminal:
                // - exit code doesn't matter
                // - stdout is injected but trailing newlines removed
                // - stderr prints to console so in our case it should be added to the root stderr
                // - It runs in its own shell, so shell vars aren't shared
                debug!("Running nested command: {:?}", cmds);
                let mut shell = Shell::new();
                shell.run_top_cmds(cmds.clone())?;

                // Add the stderr to the outer stderr, the stdout return to the caller:
                self.stderr.push_str(&shell.stderr);
                Ok(shell.stdout.trim_end().to_string())
            },
            ast::ParameterSubstitution::Alternative(..) => {
                Err(unsup("If the parameter is NOT null or unset, a provided word will be used, e.g. '${param:+[word]}'. The boolean indicates the presence of a ':', and that if the parameter has a null value, that situation should be treated as if the parameter is unset."))
            }
            ast::ParameterSubstitution::Len(_) => {
                Err(unsup(
                    "Returns the length of the value of a parameter, e.g. '${#param}'",
                ))
            }
            ast::ParameterSubstitution::Arith(_) => {
                Err(unsup(
                    "Returns the resulting value of an arithmetic substitution, e.g. '$(( x++ ))'",
                ))
            }
            ast::ParameterSubstitution::Default(_, _, _) => {
                Err(unsup(
                    "Use a provided value if the parameter is null or unset, e.g. '${param:-[word]}'. The boolean indicates the presence of a ':', and that if the parameter has a null value, that situation should be treated as if the parameter is unset.",
                ))
            }
            ast::ParameterSubstitution::Assign(_, _, _) => {
                Err(unsup(
                    "Assign a provided value to the parameter if it is null or unset, e.g. '${param:=[word]}'. The boolean indicates the presence of a ':', and that if the parameter has a null value, that situation should be treated as if the parameter is unset.",
                ))
            }
            ast::ParameterSubstitution::Error(_, _, _) => {
                Err(unsup(
                    "If the parameter is null or unset, an error should result with the provided message, e.g. '${param:?[word]}'. The boolean indicates the presence of a ':', and that if the parameter has a null value, that situation should be treated as if the parameter is unset.",
                ))
            }
            ast::ParameterSubstitution::RemoveSmallestSuffix(_, _) => Err(unsup(
                "Remove smallest suffix pattern from a parameter's value, e.g. '${param%pattern}'",
            )),
            ast::ParameterSubstitution::RemoveLargestSuffix(_, _) => Err(unsup(
                "Remove largest suffix pattern from a parameter's value, e.g. '${param%%pattern}'",
            )),
            ast::ParameterSubstitution::RemoveSmallestPrefix(_, _) => Err(unsup(
                "Remove smallest prefix pattern from a parameter's value, e.g. '${param#pattern}'",
            )),
            ast::ParameterSubstitution::RemoveLargestPrefix(_, _) => Err(unsup(
                "Remove largest prefix pattern from a parameter's value, e.g. '${param##pattern}'",
            )),
        }
    }

    /// Decide whether a tilde should be expanded to the user's home directory or not based on the surrounding context.
    /// https://www.gnu.org/software/bash/manual/html_node/Tilde-Expansion.html
    /// Above are the proper tilde rules, only implementing the basics :
    ///
    /// yes for:
    /// - ~
    /// - ~/foo
    ///
    /// no for:
    /// - ~~
    /// - foo~
    /// - ~foo
    /// - foo/~
    /// - foo~bar
    fn expand_tilde(
        &mut self,
        concat_state: Option<&'_ WordConcatState<'_>>,
        is_lookaround: bool,
    ) -> Result<bool, CmdErr> {
        // Handle infinite loop:
        if is_lookaround {
            return Ok(false);
        }

        if let Some(concat_words) = concat_state {
            // Shouldn't expand if not the first:
            if concat_words.active != 0 {
                Ok(false)
            } else if let Some(next) = concat_words.words.get(1) {
                // If the next starts with a forward slash, then should expand:
                // Marking as a lookaround so doesn't cause stack overflow and always returns false if 2 tildes in a row:
                let next_str = self.process_word(&next.clone(), concat_state, true)?;
                Ok(next_str.starts_with('/'))
            } else {
                Ok(false)
            }
        } else {
            // If its on its own then should expand:
            Ok(true)
        }
    }
}

/// Helper to create unsupported error message.
fn unsup(desc: &'static str) -> error_stack::Report<CmdErr> {
    err!(
        CmdErr::BashFeatureUnsupported,
        "Used valid bash syntax not implemented: {}",
        desc
    )
}
