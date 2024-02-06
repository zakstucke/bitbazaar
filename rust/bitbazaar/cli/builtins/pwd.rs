use crate::{
    cli::{errs::BuiltinErr, shell::Shell, CmdOut},
    prelude::*,
};

/// https://www.gnu.org/software/bash/manual/bash.html#index-pwd
pub fn pwd(shell: &mut Shell, args: &[String]) -> Result<CmdOut, BuiltinErr> {
    if !args.is_empty() {
        return Err(
            err!(BuiltinErr::Unsupported).attach_printable("pwd: options are not supported")
        );
    }

    let pwd = if let Ok(ad) = shell.active_dir() {
        ad.display().to_string()
    } else {
        return Err(
            err!(BuiltinErr::InternalError).attach_printable("pwd: failed to get active directory")
        );
    };

    Ok(CmdOut {
        stdout: format!("{}\n", pwd),
        stderr: "".to_string(),
        code: 0,
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use crate::cli::Bash;

    #[test]
    fn test_pwd() {
        // canonicalize to make absolute:
        let cur_dir = fs::canonicalize(std::env::current_dir().unwrap()).unwrap();

        // Default root should be the current dir (and absolute not relative):
        let out = Bash::new().cmd("pwd").run().unwrap();
        assert_eq!(out.code, 0);
        assert_eq!(out.stdout, format!("{}\n", cur_dir.display()));

        // Relative chdir() on bash should still not break absoluteleness of pwd (fixing bug)
        let out = Bash::new()
            .chdir(&PathBuf::from("."))
            .cmd("pwd")
            .run()
            .unwrap();
        assert_eq!(out.code, 0);
        assert_eq!(out.stdout, format!("{}\n", cur_dir.display()));
    }
}
