/// The result of running a command
#[derive(Debug, Clone)]
pub struct CmdOut {
    /// The stdout of the command:
    pub stdout: String,
    /// The stderr of the command:
    pub stderr: String,
    /// The exit code of the command:
    pub code: i32,

    /// The commands that were run, will include all commands that were attempted.
    /// I.e. if a command fails, it will be the last command in this vec, the remaining were not attempted.
    pub attempted_commands: Vec<String>,
}

impl CmdOut {
    /// Create a new CmdOut with empty stdout, stderr, and a zero exit code.
    pub(crate) fn empty() -> Self {
        Self {
            stdout: "".to_string(),
            stderr: "".to_string(),
            code: 0,
            attempted_commands: Vec::new(),
        }
    }

    /// Returns true when the command exited with a zero exit code.
    pub fn success(&self) -> bool {
        self.code == 0
    }

    /// Combines the stdout and stderr into a single string.
    pub fn std_all(&self) -> String {
        if !self.stdout.is_empty() && !self.stderr.is_empty() {
            if !self.stderr.ends_with('\n') {
                format!("{}\n{}", self.stdout, self.stderr)
            } else {
                format!("{}{}", self.stdout, self.stderr)
            }
        } else if !self.stdout.is_empty() {
            self.stdout.clone()
        } else {
            self.stderr.clone()
        }
    }

    /// Pretty format the attempted commands, with the exit code included on the final line.
    pub fn fmt_attempted_commands(&self) -> String {
        if !self.attempted_commands.is_empty() {
            let mut out = "Attempted commands:\n".to_string();
            for (index, cmd) in self.attempted_commands.iter().enumerate() {
                // Indent the commands by a bit of whitespace:
                out.push_str("   ");
                // Add cmd number:
                out.push_str(format!("{}. ", index).as_str());
                out.push_str(cmd.trim());
                // Newline if not last:
                if index < self.attempted_commands.len() - 1 {
                    out.push('\n');
                }
            }
            // On the last line, add <-- exited with code: X
            out.push_str(&format!(" <-- exited with code: {}", self.code));
            out
        } else {
            "No commands!".to_string()
        }
    }
}
