mod run_cmd;

pub use run_cmd::{run_cmd, CmdOut};

#[cfg(test)]
mod tests {
    use rstest::*;

    use super::*;
    use crate::{aer, errors::prelude::*};

    #[rstest]
    // <-- basics:
    #[case("echo 'hello world'", "hello world", 0)]
    #[case("echo hello world", "hello world", 0)]
    // <-- and"
    #[case("echo hello && echo world", "hello\nworld", 0)]
    #[case("echo hello && false && echo world", "hello", 1)]
    #[case("true && echo world", "world", 0)]
    // <-- or"
    #[case("echo hello || echo world", "hello", 0)]
    #[case("false || echo world", "world", 0)]
    #[case("false || false || echo world", "world", 0)]
    // <-- pipe:
    #[case("echo 'foo\nbar\nree' | grep -E 'foo|ree'", "foo\nree", 0)]
    #[case("echo 'foo\nbar\nree' | grep -E 'foo|ree' | wc -l", "2", 0)]
    // <-- home dir:
    #[case("echo ~", format!("{}", homedir::get_my_home().unwrap().unwrap().to_string_lossy()), 0)]
    #[case("echo ~/foo", format!("{}/foo", homedir::get_my_home().unwrap().unwrap().to_string_lossy()), 0)]
    // Should ignore home dir when not at beginning:
    #[case("echo foo~", "foo~", 0)]
    // <-- all ignored when in quotes:
    #[case("echo false '&& echo bar'", "false && echo bar", 0)]
    #[case("echo false '|| echo bar'", "false || echo bar", 0)]
    #[case("echo false '| echo bar'", "false | echo bar", 0)]
    #[case("echo '~'", "~", 0)]
    fn test_run_cmd<S: Into<String>>(
        #[case] cmd_str: &str,
        #[case] exp_std_all: S,
        #[case] code: i32,
    ) -> Result<(), AnyErr> {
        let res = aer!(run_cmd(cmd_str))?;
        assert_eq!(res.code, code, "{}", res.std_all());
        assert_eq!(res.std_all().trim(), exp_std_all.into());
        Ok(())
    }
}
