use super::bad_call;
use crate::{
    cli::{errs::BuiltinErr, shell::Shell, CmdOut},
    prelude::*,
};

/// https://www.gnu.org/software/bash/manual/bash.html#index-exit
///
/// Exit the shell, returning a status of n to the shell’s parent.
/// If n is omitted, the exit status is that of the last command executed.
/// Any trap on EXIT is executed before the shell terminates.
pub fn exit(shell: &mut Shell, args: &[String]) -> Result<CmdOut, BuiltinErr> {
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

#[cfg(test)]
mod tests {}
