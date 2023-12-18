mod run_cmd;

pub use run_cmd::{run_cmd, CmdOut};

#[cfg(test)]
mod tests {
    use rstest::*;

    use super::*;

    #[rstest]
    fn test_run_cmd() {
        let res = run_cmd("echo 'hello world'").unwrap();
        assert_eq!(res.args, &["echo", "hello world"]);
        assert_eq!(res.code, 0);
        assert_eq!(res.stdout, "hello world\n");
        assert_eq!(res.stderr, "");
    }
}
