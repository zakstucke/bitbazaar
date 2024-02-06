use std::path::PathBuf;

use normpath::PathExt;

use super::bad_call;
use crate::{
    cli::{errs::BuiltinErr, shell::Shell, CmdOut},
    prelude::*,
};

/// https://www.gnu.org/software/bash/manual/bash.html#index-cd
pub fn cd(shell: &mut Shell, args: &[String]) -> Result<CmdOut, BuiltinErr> {
    macro_rules! hd {
        () => {
            if let Ok(hd) = shell.home_dir() {
                PathBuf::from(hd.to_string_lossy().to_string())
            } else {
                bad_call!("cd: failed to get home directory")
            }
        };
    }

    let mut target_path = if let Some(last) = args.last() {
        if !last.starts_with('-') {
            PathBuf::from(last)
        } else {
            hd!()
        }
    } else {
        hd!()
    };

    let mut follow_symlinks = true;
    for (index, arg) in args.iter().enumerate() {
        match arg.as_str() {
            "-L" => follow_symlinks = true,
            "-P" => follow_symlinks = false,
            // Allow -e, but think ignore as enabled auto by this implementation
            "-e" => {}
            "-@" => {
                return Err(err!(BuiltinErr::Unsupported).attach_printable("cd: '-@' not supported"))
            }
            _ => {
                // If its the last then will be the target dir, no err:
                if index == args.len() - 1 {
                    break;
                } else {
                    // If its not the last, then shouldn't be there:
                    bad_call!("cd: invalid option: {}", arg);
                }
            }
        }
    }

    // If target_path is relative, attach onto the current dir:
    if target_path.is_relative() {
        target_path = if let Ok(ad) = shell.active_dir() {
            ad.join(target_path)
        } else {
            return Err(err!(BuiltinErr::InternalError)
                .attach_printable("cd: failed to get active directory"));
        };
    }

    // Expand symbolic links if -P is specified
    if !follow_symlinks {
        // Make absolute to expand symlinks:
        if let Ok(realpath) = target_path.normalize() {
            target_path = realpath.into_path_buf();
        } else {
            bad_call!("cd: Failed to get real path for {}", target_path.display());
        }
    }

    // Validate the path exists:
    if !target_path.exists() {
        bad_call!("cd: no such file or directory: {}", target_path.display());
    }

    // Update the shell to use the new dir:
    shell
        .chdir(target_path)
        .change_context(BuiltinErr::InternalError)?;

    Ok(CmdOut {
        stdout: "".to_string(),
        stderr: "".to_string(),
        code: 0,
    })
}

// Should be tested quite well in cli/mod.rs and other builtin tests.
#[cfg(test)]
mod tests {}
