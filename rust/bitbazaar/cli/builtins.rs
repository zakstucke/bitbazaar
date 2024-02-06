use std::{collections::HashMap, fs, path::PathBuf};

use once_cell::sync::Lazy;

use super::{errs::BuiltinErr, shell::Shell, CmdOut};
use crate::prelude::*;

/// A builtin function, implemented internally, might implement for reasons:
/// - Performance
/// - Needs to implement the pseudo rust shell
/// - Windows compatibility - all implement builtins conform to linux/mac/bash expected usage.
pub type Builtin = fn(&mut Shell, &[String]) -> Result<CmdOut, BuiltinErr>;

/// Helper for creating CmdOut with an error code and writing to stderr.
macro_rules! bad_call {
    ($($arg:tt)*) => {
        return Ok(CmdOut {
            stdout: "".to_string(),
            stderr: format!($($arg)*),
            code: 1,
        })
    };
}

pub static BUILTINS: Lazy<HashMap<&'static str, Builtin>> = Lazy::new(|| {
    let mut builtins: HashMap<&'static str, Builtin> = HashMap::new();
    builtins.insert("echo", b_echo);
    builtins.insert("cd", b_cd);
    builtins.insert("pwd", b_pwd);
    builtins.insert("exit", b_exit);
    builtins.insert("set", b_set);
    builtins
});

/// https://www.gnu.org/software/bash/manual/bash.html#index-echo
fn b_echo(_shell: &mut Shell, args: &[String]) -> Result<CmdOut, BuiltinErr> {
    let mut newline = true;

    let mut stdout = String::new();
    for (index, arg) in args.iter().enumerate() {
        match arg.as_str() {
            "-n" => newline = false,
            "-e" => {
                return Err(
                    err!(BuiltinErr::Unsupported).attach_printable("echo: '-e' not supported")
                )
            }
            "-E" => {
                return Err(
                    err!(BuiltinErr::Unsupported).attach_printable("echo: '-E' not supported")
                )
            }
            // After all recognized options, the rest are the message.
            // Add them all at once and break:
            _ => {
                stdout = args[index..].join(" ");
                break;
            }
        }
    }

    if newline {
        stdout.push('\n');
    }

    Ok(CmdOut {
        stdout,
        stderr: "".to_string(),
        code: 0,
    })
}

/// https://www.gnu.org/software/bash/manual/bash.html#index-cd
fn b_cd(shell: &mut Shell, args: &[String]) -> Result<CmdOut, BuiltinErr> {
    macro_rules! hd {
        () => {
            if let Ok(hd) = shell.home_dir() {
                PathBuf::from(hd.to_string_lossy().to_string())
            } else {
                bad_call!("cd: failed to get home directory")
            }
        };
    }

    let mut target_path = if let Some(last) = args.last() {
        if !last.starts_with('-') {
            PathBuf::from(last)
        } else {
            hd!()
        }
    } else {
        hd!()
    };

    let mut follow_symlinks = true;
    for (index, arg) in args.iter().enumerate() {
        match arg.as_str() {
            "-L" => follow_symlinks = true,
            "-P" => follow_symlinks = false,
            // Allow -e, but think ignore as enabled auto by this implementation
            "-e" => {}
            "-@" => {
                return Err(err!(BuiltinErr::Unsupported).attach_printable("cd: '-@' not supported"))
            }
            _ => {
                // If its the last then will be the target dir, no err:
                if index == args.len() - 1 {
                    break;
                } else {
                    // If its not the last, then shouldn't be there:
                    bad_call!("cd: invalid option: {}", arg);
                }
            }
        }
    }

    // If target_path is relative, attach onto the current dir:
    if target_path.is_relative() {
        target_path = if let Ok(ad) = shell.active_dir() {
            ad.join(target_path)
        } else {
            return Err(err!(BuiltinErr::InternalError)
                .attach_printable("cd: failed to get active directory"));
        };
    }

    // Expand symbolic links if -P is specified
    if !follow_symlinks {
        if let Ok(realpath) = fs::canonicalize(&target_path) {
            target_path = realpath;
        } else {
            bad_call!("cd: Failed to get real path for {}", target_path.display());
        }
    }

    // Validate the path exists:
    if !target_path.exists() {
        bad_call!("cd: no such file or directory: {}", target_path.display());
    }

    // Update the shell to use the new dir:
    shell.chdir(target_path);

    Ok(CmdOut {
        stdout: "".to_string(),
        stderr: "".to_string(),
        code: 0,
    })
}

/// https://www.gnu.org/software/bash/manual/bash.html#index-pwd
fn b_pwd(shell: &mut Shell, args: &[String]) -> Result<CmdOut, BuiltinErr> {
    if !args.is_empty() {
        return Err(
            err!(BuiltinErr::Unsupported).attach_printable("pwd: options are not supported")
        );
    }

    let pwd = if let Ok(ad) = shell.active_dir() {
        ad.display().to_string()
    } else {
        return Err(
            err!(BuiltinErr::InternalError).attach_printable("pwd: failed to get active directory")
        );
    };

    Ok(CmdOut {
        stdout: format!("{}\n", pwd),
        stderr: "".to_string(),
        code: 0,
    })
}

/// https://www.gnu.org/software/bash/manual/bash.html#index-exit
///
/// Exit the shell, returning a status of n to the shell’s parent.
/// If n is omitted, the exit status is that of the last command executed.
/// Any trap on EXIT is executed before the shell terminates.
fn b_exit(shell: &mut Shell, args: &[String]) -> Result<CmdOut, BuiltinErr> {
    let exit_code = if args.is_empty() {
        // Last code
        shell.code
    } else if let Some(code_str) = args.first() {
        match code_str.parse::<i32>() {
            Ok(code) => code,
            Err(_) => bad_call!("exit: invalid number: {}", code_str),
        }
    } else {
        bad_call!("exit: too many arguments")
    };

    // Set the code and propagate upwards:
    shell.code = exit_code;
    Err(err!(BuiltinErr::Exit))
}

/// https://www.gnu.org/software/bash/manual/html_node/The-Set-Builtin.html
fn b_set(shell: &mut Shell, args: &[String]) -> Result<CmdOut, BuiltinErr> {
    if let Some(arg) = args.first() {
        match arg.as_str() {
            "+e" => {
                shell.set_e = false;
                return Ok(CmdOut {
                    stdout: "".to_string(),
                    stderr: "".to_string(),
                    code: 0,
                });
            }
            "-e" => {
                shell.set_e = true;
                return Ok(CmdOut {
                    stdout: "".to_string(),
                    stderr: "".to_string(),
                    code: 0,
                });
            }
            _ => {}
        }
    }

    Err(err!(BuiltinErr::Unsupported).attach_printable(
        "The 'set' builtin is not fully implemented. Only 'set -e' and 'set +e' are supported.",
    ))
}
