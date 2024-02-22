mod bash;
mod builtins;
mod cmd_out;
mod errs;
mod redirect;
mod runner;
mod shell;

pub use bash::Bash;
pub use cmd_out::CmdOut;
pub use errs::BashErr;

#[cfg(test)]
mod tests {
    use normpath::PathExt;
    use once_cell::sync::Lazy;
    use rstest::*;
    use tempfile::NamedTempFile;

    use super::*;
    use crate::{errors::prelude::*, log::GlobalLog};

    #[fixture]
    fn logging() -> () {
        GlobalLog::setup_quick_stdout_global_logging(tracing::Level::DEBUG).unwrap();
    }

    // Temp file:
    fn tf() -> String {
        // Using debug formatting to make sure escaped properly on windows:
        format!("{:?}", NamedTempFile::new().unwrap().path())
    }

    static GLOB_TD: Lazy<tempfile::TempDir> = Lazy::new(|| tempfile::tempdir().unwrap());

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

    static CAT_CMD: &str = if cfg!(windows) { "cmd /c type" } else { "cat" };

    #[rstest]
    // <-- basics:
    #[case::basic_1("echo 'hello world'", "hello world", 0, None, None, true)]
    #[case::basic_2("echo hello world", "hello world", 0, None, None, true)]
    #[case::basic_3(
        "./no_exist.sh",
        if cfg!(windows) {
            "The system cannot find the file specified. (os error 2)"
        } else {
            "No such file or directory (os error 2)"
        },
        2,
        None,
        None,
        true
    )]
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
        // Exit code should be ignored from subs
        "echo foo $(echo bar && false) ree",
        "foo bar ree",
        0,
        None,
        None,
        true
    )]
    // <-- redirection:
    // Write:
    #[case::redir_1(format!("echo foo > {fp} && rm {fp}", fp=tf()), "", 0, None, None, true)]
    #[case::redir_2(format!("cd {dp:?} && echo foo > file.txt && {CAT_CMD} file.txt", dp=GLOB_TD.path()), "foo", 0, None, None, true)]
    // Write and append together:
    #[case::redir_3(
        format!("echo foo >> {fp} && echo bar > {fp} && echo ree >> {fp} && {CAT_CMD} {fp} && rm {fp}", fp=tf()),
        // Second should override the first, then final ree is appended:
        "bar\nree",
        0,
        None,
        None,
        true
    )]
    // Stdout to null:
    #[case::redir_4("echo foo >/dev/null", "", 0, None, None, true)]
    #[case::redir_5("echo foo 1>/dev/null", "", 0, None, None, true)]
    // Stdout to stderr:
    #[case::redir_6("echo foo 1>/dev/stderr", "foo", 0, Some(""), Some("foo"), true)]
    #[case::redir_7("echo foo 1>&2", "foo", 0, Some(""), Some("foo"), true)]
    #[case::redir_8("echo foo 1>/dev/fd/2", "foo", 0, Some(""), Some("foo"), true)]
    // Stderr to stdout:
    #[case::redir_9("stderr_echo foo 2>/dev/stdout", "foo", 0, Some("foo"), Some(""), true)]
    #[case::redir_10("stderr_echo foo 2>&1", "foo", 0, Some("foo"), None, true)]
    #[case::redir_11("stderr_echo foo 2>/dev/fd/1", "foo", 0, Some("foo"), Some(""), true)]
    // Read to stdin:
    #[case::redir_12(
        format!("echo foo > {fp} && <{fp} {CAT_CMD} && rm {fp}", fp=tf()),
        "foo",
        0,
        None,
        None,
        false // The windows cat variant doesn't support stdin
    )]
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
    fn test_bash_basics<S: Into<String>>(
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

        let res = Bash::new().cmd(cmd_str).run().change_context(AnyErr)?;

        assert_eq!(res.code, code, "{}: {}", res.code, res.std_all());
        if let Some(exp_stdout) = exp_stdout {
            assert_eq!(res.stdout.trim(), exp_stdout, "{}", res.std_all());
        }
        if let Some(exp_sterr) = exp_sterr {
            assert_eq!(res.stderr.trim(), exp_sterr, "{}", res.std_all());
        }
        assert_eq!(res.std_all().trim(), exp_std_all.into());
        Ok(())
    }

    /// Check multi commands are treated like lines in a file,
    /// and lines in a command are treated and work like a normal file.
    /// Set -e should be enabled by default, but can be disabled with normal set +e bash syntax.
    #[rstest]
    // Multi line files should work as expected and handle comments.
    #[case::ml_cmd(["# Start comment\necho hello # Inline comment\necho goodbye\n# End comment"], "hello\ngoodbye", 0)]
    // Multi line commands should be newline separated, i.e. as if coming in from a file.
    #[case::sim_ml_cmd(["# Start comment", "echo hello # Inline comment", "echo goodbye", "# End comment"], "hello\ngoodbye", 0)]
    #[case::set_e_on_by_default(["echo hello", "false", "echo goodbye"], "hello", 1)]
    #[case::set_e_can_be_disabled(["set +e", "echo hello", "false", "echo goodbye"], "hello\ngoodbye", 0)]
    #[case::set_e_can_be_disabled_and_re_enabled(["set +e", "set -e", "echo hello", "false", "echo goodbye"], "hello", 1)]
    fn test_bash_multiline<S: Into<String>>(
        #[case] cmds: impl Into<Vec<S>>,
        #[case] exp_std_all: S,
        #[case] code: i32,
        #[allow(unused_variables)] logging: (),
    ) -> Result<(), AnyErr> {
        let mut bash = Bash::new();
        for cmd in cmds.into() {
            bash = bash.cmd(cmd);
        }
        let res = bash.run().change_context(AnyErr)?;

        assert_eq!(res.code, code, "{}: {}", res.code, res.std_all());
        assert_eq!(res.std_all().trim(), exp_std_all.into());
        Ok(())
    }

    /// Confirm setting a custom working dir on the builder works plus when changing with cd in bash.
    #[rstest]
    fn test_run_dir(#[allow(unused_variables)] logging: ()) -> Result<(), AnyErr> {
        let temp_dir = tempfile::tempdir().change_context(AnyErr)?;
        // normalise to make sure absolute (as pwd should always be absolute)
        let temp_dir_pb = temp_dir
            .path()
            .normalize()
            .change_context(AnyErr)?
            .into_path_buf();

        // Create: ./topfile.txt
        // Create ./subdir/subfile.txt
        std::fs::write(temp_dir_pb.join("topfile.txt"), "topfile!").change_context(AnyErr)?;
        std::fs::create_dir(temp_dir_pb.join("subdir")).change_context(AnyErr)?;
        std::fs::write(temp_dir_pb.join("subdir").join("subfile.txt"), "subfile!")
            .change_context(AnyErr)?;

        let res = Bash::new()
            .chdir(&temp_dir_pb)
            .cmd("pwd")
            .cmd("cd subdir")
            .cmd("pwd")
            .cmd(format!("{} subfile.txt", CAT_CMD))
            .cmd("cd ..")
            .cmd(format!("{} topfile.txt", CAT_CMD))
            .run()
            .change_context(AnyErr)?;

        assert_eq!(res.code, 0, "{}: {}", res.code, res.std_all());
        assert_eq!(
            res.stdout.trim(),
            format!(
                "{}\n{}\nsubfile!topfile!",
                temp_dir_pb.display(),
                temp_dir_pb.join("subdir").display()
            )
        );
        Ok(())
    }

    /// Confirm setting env vars on the builder work.
    #[rstest]
    fn test_builder_env(#[allow(unused_variables)] logging: ()) -> Result<(), AnyErr> {
        let res = Bash::new()
            .env("FOO", "bar")
            .env("BAZ", "qux")
            .cmd("echo $FOO $(echo $BAZ)")
            .run()
            .change_context(AnyErr)?;
        assert_eq!(res.code, 0, "{}: {}", res.code, res.std_all());
        assert_eq!(res.stdout.trim(), format!("bar qux"));
        Ok(())
    }

    // Confirm when both when doesn't error but not all commands run AND when Bash errors the final command that was attempted is accessible and printable.
    #[rstest]
    fn test_error_source_attached(#[allow(unused_variables)] logging: ()) -> Result<(), AnyErr> {
        let err_cmd = "ab||][/?cd";

        // Confirm that when bash itself fails (i.e. invalid syntax), the source is attached to the error:
        let res = Bash::new()
            .cmd("echo foo")
            .cmd(err_cmd)
            .cmd("echo bar")
            .run();
        assert!(res.is_err());
        let fmted = format!("{:?}", res.as_ref().unwrap_err());
        assert!(
            fmted.contains(format!("{} <-- exited with code:", err_cmd).as_str()),
            "{}",
            fmted
        );

        // Confirm cmd out is attached and the source could be inferred from there:
        let e = res.unwrap_err();
        let cmd_out = e.current_context().cmd_out();
        assert_eq!(cmd_out.attempted_commands.len(), 2);
        assert_eq!(cmd_out.attempted_commands[1], err_cmd);

        // Now confirm when it's a valid set of commands, but one just fails, also accessible:
        let cmd_out = Bash::new()
            .cmd("echo foo")
            .cmd("echo bar && false")
            .cmd("echo bar")
            .run()
            .unwrap();
        assert_eq!(cmd_out.attempted_commands.len(), 2);
        assert_eq!(cmd_out.attempted_commands[1], "echo bar && false");

        Ok(())
    }
}
