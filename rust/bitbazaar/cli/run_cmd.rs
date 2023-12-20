use crate::{err, errors::TracedErr};

/// The result of running a command
pub struct CmdOut {
    /// The parsed arguments from the command string used in the call:
    pub args: Vec<String>,
    /// The stdout of the command:
    pub stdout: String,
    /// The stderr of the command:
    pub stderr: String,
    /// The exit code of the command:
    pub code: i32,
}

/// Run a command entered as a string and return the output
pub fn run_cmd(cmd_str: &str) -> Result<CmdOut, TracedErr> {
    // Split the string into args, shlex handles posix rules such as keeping quotes and speech marks together automatically:
    let args = shlex::split(cmd_str).ok_or_else(|| err!("Failed to parse command string"))?;

    if args.is_empty() {
        return Err(err!("Empty command string"));
    }

    // Create a new Command
    let mut command = std::process::Command::new(&args[0]);

    if args.len() > 1 {
        // Add arguments to the command
        command.args(&args[1..]);
    }

    let output = command.output().map_err(|e| {
        err!(
            "Command returned non-zero exit status '{}'.\nCommand: '{}'.\n'Err: '{}'",
            e.raw_os_error().unwrap_or(-1),
            cmd_str,
            e
        )
    })?;

    let stdout = String::from_utf8(output.stdout).unwrap_or("Decoding stdout failed".to_string());
    let stderr = String::from_utf8(output.stderr).unwrap_or("Decoding stderr failed".to_string());

    Ok(CmdOut {
        args,
        stdout,
        stderr,
        code: output
            .status
            .code()
            .ok_or_else(|| err!("Command returned no exit status"))?,
    })
}
