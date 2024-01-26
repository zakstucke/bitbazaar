mod bash;
mod cmd_err;
mod cmd_out;
mod runner;

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

    static WC_CMD: &str = if cfg!(windows) {
        "find /c /v \"\""
    } else {
        "wc -l"
    };

    static CAT_CMD: &str = if cfg!(windows) { "type" } else { "cat" };

    #[rstest]
    // <-- basics:
    #[case::basic_1("echo 'hello world'", "hello world", 0, None, None, true)]
    #[case::basic_2("echo hello world", "hello world", 0, None, None, true)]
    #[case::basic_3("./no_exist.sh", "", 2, None, None, true)]
    // <-- and:
    #[case::and_1("echo hello && echo world", "hello\nworld", 0, None, None, true)]
    #[case::and_2("echo hello && false && echo world", "hello", 1, None, None, true)]
    #[case::and_3("true && echo world", "world", 0, None, None, true)]
    // <-- or:
    #[case::or_1("echo hello || echo world", "hello", 0, None, None, true)]
    #[case::or_2("false || echo world", "world", 0, None, None, true)]
    #[case::or_3("false || false || echo world", "world", 0, None, None, true)]
    // <-- negations:
    #[case::neg_1("! echo hello || echo world", "hello\nworld", 0, None, None, true)]
    // <-- pipe:
    #[case::pipe_1(format!("echo foo | {WC_CMD}"), "1", 0, None, None, true)]
    #[case::pipe_2(
        // Looks weird because avoiding newlines to work with windows: (but also tests compound)
        format!("(echo foo && echo ree && echo bar) | {WC_CMD}"),
        "3",
        0,
        None,
        None,
        true
    )]
    #[case::pipe_3(
        // Looks weird because avoiding newlines to work with windows: (but also tests compound)
        format!("(echo foo && echo ree && echo bar) | {WC_CMD} | {WC_CMD}"),
        "1",
        0,
        None,
        None,
        true
    )]
    // <-- stderr coming through:
    #[case::stderr_1(
        format!("{CAT_CMD} non_existent.txt || echo foo && {CAT_CMD} ree.txt"),
        if cfg!(windows) {
            "foo\nThe system cannot find the file specified.\nThe system cannot find the file specified."
        } else {
            "foo\ncat: non_existent.txt: No such file or directory\ncat: ree.txt: No such file or directory"
        },
        1,
        Some("foo"),
        Some(if cfg!(windows){
            "The system cannot find the file specified.\nThe system cannot find the file specified."
        } else {
            "cat: non_existent.txt: No such file or directory\ncat: ree.txt: No such file or directory"
        }),
        true
    )]
    // <-- command substitution:
    #[case::subst_1("echo $(echo foo)", "foo", 0, None, None, true)]
    #[case::subst_2("echo $(echo foo) $(echo bar)", "foo bar", 0, None, None, true)]
    #[case::subst_3("echo foo $(echo bar) ree", "foo bar ree", 0, None, None, true)]
    #[case::subst_4(
        "echo foo $(echo bar && false) ree",
        "foo bar ree",
        0,
        None,
        None,
        true
    )] // Exit code should be ignored from subs
    #[case::subst_5(
        "echo foo $(echo bar && exit 1) ree",
        "foo bar ree",
        0,
        None,
        None,
        true
    )] // Exit code should be ignored from subs
    // <-- home dir (tilde):
    #[case::home_1("echo ~", format!("{}", home()), 0, None, None, true)]
    #[case::home_2("echo ~ ~", format!("{} {}", home(), home()), 0, None, None, true)]
    #[case::home_3("echo ~/foo", format!("{}/foo", home()), 0, None, None, true)]
    // <-- params, should be settable, stick to their current shell etc:
    #[case::home_4(
        // First should print nothing, as not set yet, gets set to 1 in outer shell, 2 in inner shell
        "echo -n \"before.$LAH. \"; LAH=1; echo outer.$LAH. $(LAH=2; echo inner.$LAH.) outer.$LAH.",
        "before.. outer.1. inner.2. outer.1.",
        0, None, None, true
    )]
    // Should ignore tilde in most circumstances:
    #[case::home_5("echo ~~", "~~", 0, None, None, true)]
    #[case::home_6("echo foo~", "foo~", 0, None, None, true)]
    #[case::home_7("echo ~foo", "~foo", 0, None, None, true)]
    #[case::home_8("echo foo/~", "foo/~", 0, None, None, true)]
    #[case::home_9("echo foo~bar", "foo~bar", 0, None, None, true)]
    // Don't test on windows this one, the OS seems to override and convert after leaving rust:
    #[case::home_10("echo \"~\"", "~", 0, None, None, false)]
    #[case::home_11("echo \"~/foo\"", "~/foo", 0, None, None, true)]
    // <-- all ignored when in quotes:
    #[case::lit_1("echo false '&& echo bar'", "false && echo bar", 0, None, None, true)]
    #[case::lit_2("echo false '|| echo bar'", "false || echo bar", 0, None, None, true)]
    #[case::lit_3("echo false '| echo bar'", "false | echo bar", 0, None, None, true)]
    #[case::lit_4("echo false '$(echo bar)'", "false $(echo bar)", 0, None, None, true)]
    // Don't test on windows this one, the OS seems to override and convert after leaving rust:
    #[case::lit_5("echo '~'", "~", 0, None, None, false)]
    fn test_execute_bash<S: Into<String>>(
        #[case] cmd_str: String,
        #[case] exp_std_all: S,
        #[case] code: i32,
        #[case] exp_stdout: Option<&str>, // Only check if Some()
        #[case] exp_sterr: Option<&str>,  // Only check if Some()
        #[case] test_on_windows: bool,    // Only check if Some()
        #[allow(unused_variables)] logging: (),
    ) -> Result<(), AnyErr> {
        if cfg!(windows) && !test_on_windows {
            return Ok(());
        }

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
