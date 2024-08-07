mod cd;
mod echo;
mod exit;
mod pwd;
mod set;

use std::{collections::HashMap, sync::LazyLock};

use super::{errs::BuiltinErr, shell::Shell, BashOut};
use crate::prelude::*;

/// A builtin function, implemented internally, might implement for reasons:
/// - Performance
/// - Needs to implement the pseudo rust shell
/// - Windows compatibility - all implement builtins conform to linux/mac/bash expected usage.
pub type Builtin = fn(&mut Shell, &[String]) -> RResult<BashOut, BuiltinErr>;

/// Helper for creating BashOut with an error code and writing to stderr.
macro_rules! bad_call {
    ($($arg:tt)*) => {
        return Ok(CmdResult::new("", 1, "", format!($($arg)*)).into())
    };
}

pub(crate) use bad_call;

pub static BUILTINS: LazyLock<HashMap<&'static str, Builtin>> = LazyLock::new(|| {
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
fn std_err_echo(_shell: &mut Shell, args: &[String]) -> RResult<BashOut, BuiltinErr> {
    use super::CmdResult;

    Ok(CmdResult::new("", 0, "", args.join(" ")).into())
}
