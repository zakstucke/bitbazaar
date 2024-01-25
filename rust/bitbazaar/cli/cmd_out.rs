/// The result of running a command
pub struct CmdOut {
    /// The stdout of the command:
    pub stdout: String,
    /// The stderr of the command:
    pub stderr: String,
    /// The exit code of the command:
    pub code: i32,
}

impl CmdOut {
    /// Returns true when the command exited with a zero exit code.
    pub fn success(&self) -> bool {
        self.code == 0
    }

    /// Combines the stdout and stderr into a single string.
    pub fn std_all(&self) -> String {
        if !self.stdout.is_empty() && !self.stderr.is_empty() {
            format!("{}\n{}", self.stdout, self.stderr)
        } else if !self.stdout.is_empty() {
            self.stdout.clone()
        } else {
            self.stderr.clone()
        }
    }
}

impl CmdOut {
    pub(crate) fn new() -> Self {
        Self {
            stdout: String::new(),
            stderr: String::new(),
            code: 0,
        }
    }

    pub(crate) fn merge(&mut self, other: CmdOut) {
        self.stdout.push_str(&other.stdout);
        self.stderr.push_str(&other.stderr);
        self.code = other.code;
    }
}