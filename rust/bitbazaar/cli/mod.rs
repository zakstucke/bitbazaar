mod bash;
mod cmd_err;
mod cmd_out;
pub use bash::execute_bash;
pub use cmd_err::CmdErr;
pub use cmd_out::CmdOut;
#[cfg(test)]
mod tests {
    use once_cell::sync::Lazy;
    use rstest::*;

    use super::*;
    use crate::{errors::prelude::*, logging::default_stdout_global_logging};

    #[fixture]
    fn logging() -> () {
        default_stdout_global_logging(tracing::Level::DEBUG).unwrap();
    }

    static HOME_DIR: Lazy<String> = Lazy::new(|| {
        homedir::get_my_home()
            .unwrap()
            .unwrap()
            .to_string_lossy()
            .to_string()
    });

    fn home() -> String {
        HOME_DIR.clone()
    }

    #[rstest]
    // <-- basics:
    #[case("echo 'hello world'", "hello world", 0, None, None)]
    #[case("echo hello world", "hello world", 0, None, None)]
    // <-- and:
    #[case("echo hello && echo world", "hello\nworld", 0, None, None)]
    #[case("echo hello && false && echo world", "hello", 1, None, None)]
    #[case("true && echo world", "world", 0, None, None)]
    // <-- or:
    #[case("echo hello || echo world", "hello", 0, None, None)]
    #[case("false || echo world", "world", 0, None, None)]
    #[case("false || false || echo world", "world", 0, None, None)]
    // <-- negations:
    #[case("! echo hello || echo world", "hello\nworld", 0, None, None)]
    // <-- pipe:
    #[case("echo 'foo\nbar\nree' | grep -E 'foo|ree'", "foo\nree", 0, None, None)]
    #[case("echo 'foo\nbar\nree' | grep -E 'foo|ree' | wc -l", "2", 0, None, None)]
    // <-- command substitution:
    #[case("echo $(echo foo)", "foo", 0, None, None)]
    #[case("echo $(echo foo) $(echo bar)", "foo bar", 0, None, None)]
    #[case("echo foo $(echo bar) ree", "foo bar ree", 0, None, None)]
    #[case("echo foo $(echo bar && exit 1) ree", "foo bar ree", 0, None, None)] // Exit code should be ignored from subs
    // <-- home dir (tilde):
    #[case("echo ~", format!("{}", home()), 0, None, None)]
    #[case("echo ~ ~", format!("{} {}", home(), home()), 0, None, None)]
    #[case("echo ~/foo", format!("{}/foo", home()), 0, None, None)]
    // <-- params, should be settable, stick to their current shell etc:
    #[case(
        // First should print nothing, as not set yet, gets set to 1 in outer shell, 2 in inner shell
        "echo -n \"before.$LAH. \"; LAH=1; echo outer.$LAH. $(LAH=2; echo inner.$LAH.) outer.$LAH.",
        "before.. outer.1. inner.2. outer.1.",
        0, None, None
    )]
    // Should ignore tilde in most circumstances:
    #[case("echo ~~", "~~", 0, None, None)]
    #[case("echo foo~", "foo~", 0, None, None)]
    #[case("echo ~foo", "~foo", 0, None, None)]
    #[case("echo foo/~", "foo/~", 0, None, None)]
    #[case("echo foo~bar", "foo~bar", 0, None, None)]
    #[case("echo \"~\"", "~", 0, None, None)]
    #[case("echo \"~/foo\"", "~/foo", 0, None, None)]
    // <-- all ignored when in quotes:
    #[case("echo false '&& echo bar'", "false && echo bar", 0, None, None)]
    #[case("echo false '|| echo bar'", "false || echo bar", 0, None, None)]
    #[case("echo false '| echo bar'", "false | echo bar", 0, None, None)]
    #[case("echo false '$(echo bar)'", "false $(echo bar)", 0, None, None)]
    #[case("echo '~'", "~", 0, None, None)]
    fn test_execute_bash<S: Into<String>>(
        #[case] cmd_str: &str,
        #[case] exp_std_all: S,
        #[case] code: i32,
        #[case] exp_stdout: Option<&str>, // Only check if Some()
        #[case] exp_sterr: Option<&str>,  // Only check if Some()
        #[allow(unused_variables)] logging: (),
    ) -> Result<(), AnyErr> {
        let res = execute_bash(cmd_str).change_context(AnyErr)?;
        assert_eq!(res.code, code, "{}", res.std_all());

        if let Some(exp_stdout) = exp_stdout {
            assert_eq!(res.stdout.trim(), exp_stdout);
        }
        if let Some(exp_sterr) = exp_sterr {
            assert_eq!(res.stderr.trim(), exp_sterr);
        }

        assert_eq!(res.std_all().trim(), exp_std_all.into());
        Ok(())
    }
}
