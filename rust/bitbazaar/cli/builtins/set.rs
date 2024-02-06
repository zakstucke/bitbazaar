use crate::{
    cli::{errs::BuiltinErr, shell::Shell, CmdOut},
    prelude::*,
};

/// https://www.gnu.org/software/bash/manual/html_node/The-Set-Builtin.html
pub fn set(shell: &mut Shell, args: &[String]) -> Result<CmdOut, BuiltinErr> {
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

#[cfg(test)]
mod tests {}
