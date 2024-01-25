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

    static GREP_CMD: &str = if cfg!(windows) {
        "findstr /R \"foo bar\""
    } else {
        "grep -E 'foo|bar'"
    };

    static WC_CMD: &str = if cfg!(windows) {
        "find /c /v \"\""
    } else {
        "wc -l"
    };

    #[rstest]
    // <-- basics:
    #[case::basic_1("echo 'hello world'", "hello world", 0, None, None)]
    #[case::basic_2("echo hello world", "hello world", 0, None, None)]
    // <-- and:
    #[case::and_1("echo hello && echo world", "hello\nworld", 0, None, None)]
    #[case::and_2("echo hello && false && echo world", "hello", 1, None, None)]
    #[case::and_3("true && echo world", "world", 0, None, None)]
    // <-- or:
    #[case::or_1("echo hello || echo world", "hello", 0, None, None)]
    #[case::or_2("false || echo world", "world", 0, None, None)]
    #[case::or_3("false || false || echo world", "world", 0, None, None)]
    // <-- negations:
    #[case::neg_1("! echo hello || echo world", "hello\nworld", 0, None, None)]
    // <-- pipe:
    #[case::pipe_1(format!("echo foo | {WC_CMD}"), "1", 0, None, None)]
    #[case::pipe_2(
        // Looks weird because avoiding newlines to work with windows: (but also tests compound)
        format!("(echo foo && echo ree && echo bar) | {GREP_CMD}"),
        "foo\nbar",
        0,
        None,
        None
    )]
    #[case::pipe_3(
        // Looks weird because avoiding newlines to work with windows in CI: (but also tests compound)
        format!("(echo foo && echo ree && echo bar) | {GREP_CMD} | {WC_CMD}"),
        "2",
        0,
        None,
        None
    )]
    // <-- command substitution:
    #[case::subst_1("echo $(echo foo)", "foo", 0, None, None)]
    #[case::subst_2("echo $(echo foo) $(echo bar)", "foo bar", 0, None, None)]
    #[case::subst_3("echo foo $(echo bar) ree", "foo bar ree", 0, None, None)]
    #[case::subst_4("echo foo $(echo bar && false) ree", "foo bar ree", 0, None, None)] // Exit code should be ignored from subs
    #[case::subst_5("echo foo $(echo bar && exit 1) ree", "foo bar ree", 0, None, None)] // Exit code should be ignored from subs
    // <-- home dir (tilde):
    #[case::home_1("echo ~", format!("{}", home()), 0, None, None)]
    #[case::home_2("echo ~ ~", format!("{} {}", home(), home()), 0, None, None)]
    #[case::home_3("echo ~/foo", format!("{}/foo", home()), 0, None, None)]
    // <-- params, should be settable, stick to their current shell etc:
    #[case::home_4(
        // First should print nothing, as not set yet, gets set to 1 in outer shell, 2 in inner shell
        "echo -n \"before.$LAH. \"; LAH=1; echo outer.$LAH. $(LAH=2; echo inner.$LAH.) outer.$LAH.",
        "before.. outer.1. inner.2. outer.1.",
        0, None, None
    )]
    // Should ignore tilde in most circumstances:
    #[case::home_5("echo ~~", "~~", 0, None, None)]
    #[case::home_6("echo foo~", "foo~", 0, None, None)]
    #[case::home_7("echo ~foo", "~foo", 0, None, None)]
    #[case::home_8("echo foo/~", "foo/~", 0, None, None)]
    #[case::home_9("echo foo~bar", "foo~bar", 0, None, None)]
    #[case::home_10("echo \"~\"", "~", 0, None, None)]
    #[case::home_11("echo \"~/foo\"", "~/foo", 0, None, None)]
    // <-- all ignored when in quotes:
    #[case::lit_1("echo false '&& echo bar'", "false && echo bar", 0, None, None)]
    #[case::lit_2("echo false '|| echo bar'", "false || echo bar", 0, None, None)]
    #[case::lit_3("echo false '| echo bar'", "false | echo bar", 0, None, None)]
    #[case::lit_4("echo false '$(echo bar)'", "false $(echo bar)", 0, None, None)]
    #[case::lit_5("echo '~'", "~", 0, None, None)]
    fn test_execute_bash<S: Into<String>>(
        #[case] cmd_str: String,
        #[case] exp_std_all: S,
        #[case] code: i32,
        #[case] exp_stdout: Option<&str>, // Only check if Some()
        #[case] exp_sterr: Option<&str>,  // Only check if Some()
        #[allow(unused_variables)] logging: (),
    ) -> Result<(), AnyErr> {
        let res = execute_bash(&cmd_str).change_context(AnyErr)?;
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
