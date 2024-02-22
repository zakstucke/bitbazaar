use crate::{
    cli::{errs::BuiltinErr, shell::Shell, BashOut, CmdResult},
    prelude::*,
};

/// https://www.gnu.org/software/bash/manual/bash.html#index-pwd
pub fn pwd(shell: &mut Shell, args: &[String]) -> Result<BashOut, BuiltinErr> {
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

    Ok(CmdResult::new("", 0, format!("{}\n", pwd), "").into())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use normpath::PathExt;

    use crate::cli::Bash;

    #[test]
    fn test_pwd() {
        // Make sure made absolute with normalize:
        let cur_dir = std::env::current_dir().unwrap().normalize().unwrap();

        // Default root should be the current dir (and absolute not relative):
        let out = Bash::new().cmd("pwd").run().unwrap();
        assert_eq!(out.code(), 0);
        assert_eq!(
            out.stdout(),
            format!("{}\n", cur_dir.as_os_str().to_string_lossy())
        );

        // Relative chdir() on bash should still not break absoluteleness of pwd (fixing bug)
        let out = Bash::new()
            .chdir(&PathBuf::from("."))
            .cmd("pwd")
            .run()
            .unwrap();
        assert_eq!(out.code(), 0);
        assert_eq!(
            out.stdout(),
            format!("{}\n", cur_dir.as_os_str().to_string_lossy())
        );
    }
}
