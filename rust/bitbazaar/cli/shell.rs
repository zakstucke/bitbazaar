use std::{collections::HashMap, mem, path::PathBuf, str};

use conch_parser::{ast, lexer::Lexer, parse::DefaultParser};
use normpath::PathExt;

use super::{errs::ShellErr, runner::PipeRunner, BashOut, CmdResult};
use crate::prelude::*;

#[derive(Debug)]
struct WordConcatState<'a> {
    active: usize,
    words: &'a Vec<ast::DefaultWord>,
}

pub struct Shell {
    // Finalised output that won't be piped to another command and should be returned to the caller:
    // This is only populated at the top level with the public execute_command_strings() method.
    pub cmd_results: Vec<CmdResult>,
    root_dir: Option<PathBuf>,
    /// Extra params/env vars added to this shell
    pub vars: HashMap<String, String>,
    pub set_e: bool,
    // Each executed command string supplied will be added here. Will be here even if the command fails.
    // Only commands that weren't tried due to previous problems will be missing.
    pub attempted_command_strings: Vec<String>,

    // Current in process results, at the top level these will be added to cmd_results.
    stdout: String,
    stderr: String,
    code: i32,
}

impl From<Shell> for BashOut {
    fn from(val: Shell) -> Self {
        let mut results = val.cmd_results;
        // For the subshells, these don't use cmd_results and theirs will all be in the buffers:
        if !val.stdout.is_empty()
            || !val.stderr.is_empty()
            || (Some(val.code) != results.last().map(|r| r.code))
        {
            results.push(CmdResult::new("", val.code, val.stdout, val.stderr));
        }
        BashOut::new(results)
    }
}

impl Shell {
    pub fn new(env: HashMap<String, String>, root_dir: Option<PathBuf>) -> RResult<Self, ShellErr> {
        let mut shell = Self {
            cmd_results: Vec::new(),
            root_dir: None,
            vars: env,
            // By default have set -e enabled to break if a line errors:
            set_e: true,
            attempted_command_strings: Vec::new(),
            stdout: String::new(),
            stderr: String::new(),
            code: 0,
        };

        // Chdir() does some normalisation logic, so using that rather than just setting to shell above directly:
        if let Some(root_dir) = root_dir {
            shell.chdir(root_dir)?;
        }

        Ok(shell)
    }

    pub fn execute_command_strings(&mut self, commands: Vec<String>) -> RResult<(), ShellErr> {
        // Whilst all commands could be given to the parser together (newline separated),
        // and run internally by the shell in a single function call,
        // that mean's the source command string that causes an issues would be lost
        // (e.g. an actual ShellErr OR a non zero exit code stopping further commands from running)
        for cmd_source in commands {
            // Add the result object before anything else:
            self.cmd_results.push(CmdResult::new(
                cmd_source.clone(),
                self.code,
                "".to_string(),
                "".to_string(),
            ));

            // Add the command before hitting anything that could fail:
            self.attempted_command_strings.push(cmd_source.clone());

            let lex = Lexer::new(cmd_source.chars());
            let parser = DefaultParser::new(lex);

            let parsed_top_cmds = parser
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
                .change_context(ShellErr::BashSyntaxError)?;

            // Run the command:
            let result = self.run_top_cmds(parsed_top_cmds);
            // Extract the stdout, stderr and code from the shell and add it to the new result, no matter what happened:
            let cmd_result = self.cmd_results.last_mut().unwrap();
            cmd_result.code = self.code;
            cmd_result.stdout = std::mem::take(&mut self.stdout);
            cmd_result.stderr = std::mem::take(&mut self.stderr);

            // Handle actual shell errors (not code errors, problems parsing etc)
            if let Err(e) = result {
                match e.current_context() {
                    // Exits shouldn't propagate outside a shell, the handler will have already set the proper code to the shell.
                    ShellErr::Exit => {}
                    _ => {
                        return Err(e);
                    }
                }
            }

            // Might need to stop based on the code:
            if !self.should_continue() {
                break;
            }
        }

        Ok(())
    }

    pub fn push_stdout(&mut self, stdout: &str) {
        #[cfg(windows)]
        // Need to clean on windows:
        self.stdout.push_str(&stdout.replace("\r\n", "\n"));

        #[cfg(not(windows))]
        self.stdout.push_str(stdout);
    }

    pub fn push_stderr(&mut self, stderr: &str) {
        #[cfg(windows)]
        // Need to clean on windows:
        self.stderr.push_str(&stderr.replace("\r\n", "\n"));

        #[cfg(not(windows))]
        self.stderr.push_str(stderr);
    }

    pub fn set_code(&mut self, code: i32) {
        self.code = code;
    }

    pub fn code(&self) -> i32 {
        self.code
    }

    pub fn active_dir(&self) -> RResult<PathBuf, ShellErr> {
        if let Some(root_dir) = &self.root_dir {
            Ok(root_dir.clone())
        } else {
            // Get process current dir:
            std::env::current_dir().change_context(ShellErr::InternalError)
        }
    }

    pub fn chdir(&mut self, new_root_dir: PathBuf) -> RResult<(), ShellErr> {
        // normalise to ensure its absolute (to not break e.g. pwd)
        self.root_dir = Some(
            new_root_dir
                .normalize()
                .change_context(ShellErr::InternalError)?
                .into_path_buf(),
        );
        Ok(())
    }

    /// Returns false when the code isn't 0 and set -e is enabled.
    fn should_continue(&self) -> bool {
        // Don't continue if set -e is enabled and the last command failed:
        #[allow(clippy::needless_bool)]
        if self.code() != 0 && self.set_e {
            false
        } else {
            true
        }
    }

    fn run_top_cmds(&mut self, cmds: Vec<ast::TopLevelCommand<String>>) -> RResult<(), ShellErr> {
        // Each res equates to a line in a multi line bash script. E.g. a single line command will only have one res.
        for cmd in cmds {
            match cmd.0 {
                ast::Command::Job(job) => {
                    return Err(err!(
                        ShellErr::BashFeatureUnsupported,
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
                                if self.code() == 0 {
                                    self.run_listable_command(cmd)?;
                                }
                            }
                            ast::AndOr::Or(cmd) => {
                                // Only run if the last didn't succeed:
                                if self.code() != 0 {
                                    self.run_listable_command(cmd)?;
                                }
                            }
                        }
                    }
                }
            }

            if !self.should_continue() {
                break;
            }
        }

        Ok(())
    }

    fn run_listable_command(&mut self, cmd: ast::DefaultListableCommand) -> RResult<(), ShellErr> {
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
    ) -> RResult<(), ShellErr> {
        match cmd {
            ast::PipeableCommand::Simple(cmd) => self.add_simple_command(pipe_runner, cmd)?,
            ast::PipeableCommand::Compound(compound) => {
                // E.g. (echo foo && echo bar)
                match &compound.kind {
                    ast::CompoundCommandKind::Subshell(sub_cmds) => {
                        let mut shell = Shell::new(self.vars.clone(), self.root_dir.clone())?;
                        shell.run_top_cmds(sub_cmds.clone())?;
                        let out: BashOut = shell.into();

                        // Add the stderr to the current shell:
                        self.push_stderr(&out.stderr());

                        // Add the pre-computed stdout to be used as stdin to the next command in the outer runner:
                        pipe_runner.add_piped_stdout(out.stdout());
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
                ShellErr::BashFeatureUnsupported,
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
    ) -> RResult<(), ShellErr> {
        let mut env = Vec::<(String, String)>::new();

        for item in cmd.redirects_or_env_vars.iter() {
            match item {
                ast::RedirectOrEnvVar::Redirect(redirect) => pipe_runner.add_redirect(redirect)?,
                ast::RedirectOrEnvVar::EnvVar(name, val) => {
                    let value = if let Some(val) = val {
                        self.process_complex_word(&val.0)?
                    } else {
                        "".to_string()
                    };
                    debug!("Setting env var: '{}'='{}'", name, value);
                    env.push((name.to_string(), value));
                }
            }
        }

        // Add the env vars to the current shell to in this command, later and parser expansions etc:
        for (name, val) in env.iter() {
            self.vars.insert(name.to_string(), val.to_string());
        }

        let mut args = vec![];
        for arg in cmd.redirects_or_cmd_words.iter() {
            match arg {
                ast::RedirectOrCmdWord::CmdWord(word) => {
                    args.push(self.process_complex_word(&word.0)?)
                }
                ast::RedirectOrCmdWord::Redirect(redirect) => {
                    // A redirect occurring, split off into 2 commands surrounding the redirect:
                    let args_partial = mem::take(&mut args);
                    pipe_runner.add(args_partial)?;
                    pipe_runner.add_redirect(redirect)?;
                }
            }
        }

        // Only add final if args exist:
        if !args.is_empty() {
            pipe_runner.add(args)?;
        };

        Ok(())
    }

    pub fn process_complex_word(
        &mut self,
        word: &ast::DefaultComplexWord,
    ) -> RResult<String, ShellErr> {
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
                    .collect::<RResult<Vec<_>, _>>()?
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
    ) -> RResult<String, ShellErr> {
        Ok(match word {
            // Single quoted means no processing inside needed:
            ast::Word::SingleQuoted(word) => word.to_string(),
            ast::Word::Simple(word) => {
                self.process_simple_word(word, concat_state, is_lookaround)?
            }
            ast::Word::DoubleQuoted(words) => words
                .iter()
                .map(|word| self.process_simple_word(word, concat_state, is_lookaround))
                .collect::<RResult<Vec<_>, _>>()?
                .into_iter()
                .collect::<Vec<_>>()
                .join(""),
        })
    }

    pub fn home_dir(&self) -> RResult<PathBuf, ShellErr> {
        homedir::get_my_home()
            .change_context(ShellErr::InternalError)?
            .ok_or_else(|| err!(ShellErr::InternalError))
    }

    fn process_simple_word(
        &mut self,
        word: &ast::DefaultSimpleWord,
        concat_state: Option<&'_ WordConcatState<'_>>,
        is_lookaround: bool,
    ) -> RResult<String, ShellErr> {
        Ok(match word {
            ast::SimpleWord::Literal(lit) => lit.to_string(),
            ast::SimpleWord::Escaped(a) => a.to_string(),
            ast::SimpleWord::Tilde => {
                if self.expand_tilde(concat_state, is_lookaround)? {
                    // Convert to the user's home directory:
                    self.home_dir()?.to_string_lossy().to_string()
                } else {
                    "~".to_string()
                }
            }
            ast::SimpleWord::Param(param) => self.process_param(param)?,
            ast::SimpleWord::Subst(sub) => self.process_substitution(sub)?,
            // Colon does have some special meaning, but not currently supporting and also has normal meaning (e.g. on windows), so leaving as is:
            ast::SimpleWord::Colon => ":".to_string(),
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

    fn process_param(&mut self, param: &ast::DefaultParameter) -> RResult<String, ShellErr> {
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
    ) -> RResult<String, ShellErr> {
        match sub {
            ast::ParameterSubstitution::Command(cmds) => {
                // Run the nested command, from my tests with terminal:
                // - exit code doesn't matter
                // - stdout is injected but trailing newlines removed
                // - stderr prints to console so in our case it should be added to the root stderr
                // - It runs in its own shell, so shell vars aren't shared
                debug!("Running nested command: {:?}", cmds);
                let mut shell = Shell::new(self.vars.clone(), self.root_dir.clone())?;
                shell.run_top_cmds(cmds.clone())?;
                let out: BashOut = shell.into();

                // Add the stderr to the outer stderr, the stdout return to the caller:
                self.push_stderr(&out.stderr());
                Ok(out.stdout().trim_end().to_string())
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
    ) -> RResult<bool, ShellErr> {
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
fn unsup(desc: &'static str) -> Report<ShellErr> {
    err!(
        ShellErr::BashFeatureUnsupported,
        "Used valid bash syntax not implemented: {}",
        desc
    )
}
