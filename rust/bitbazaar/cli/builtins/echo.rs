use crate::{
    cli::{errs::BuiltinErr, shell::Shell, BashOut, CmdResult},
    prelude::*,
};

/// https://www.gnu.org/software/bash/manual/bash.html#index-echo
pub fn echo(_shell: &mut Shell, args: &[String]) -> Result<BashOut, BuiltinErr> {
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

    Ok(CmdResult::new("", 0, stdout, "").into())
}

#[cfg(test)]
mod tests {}
