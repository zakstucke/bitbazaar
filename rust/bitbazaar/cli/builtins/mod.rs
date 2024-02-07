mod cd;
mod echo;
mod exit;
mod pwd;
mod set;

use std::collections::HashMap;

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

pub(crate) use bad_call;

pub static BUILTINS: Lazy<HashMap<&'static str, Builtin>> = Lazy::new(|| {
    let mut builtins: HashMap<&'static str, Builtin> = HashMap::new();
    builtins.insert("echo", echo::echo);
    builtins.insert("cd", cd::cd);
    builtins.insert("pwd", pwd::pwd);
    builtins.insert("exit", exit::exit);
    builtins.insert("set", set::set);

    #[cfg(test)]
    builtins.insert("stderr_echo", std_err_echo);

    builtins
});

#[cfg(test)]
fn std_err_echo(_shell: &mut Shell, args: &[String]) -> Result<CmdOut, BuiltinErr> {
    Ok(CmdOut {
        stdout: "".to_string(),
        stderr: args.join(" "),
        code: 0,
    })
}
