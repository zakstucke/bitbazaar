use std::{future::Future, process::Stdio, sync::Arc};

use futures::future::BoxFuture;

/// Extension trait for getting the spawn extension builder from both std and tokio commands.
pub trait CmdSpawnExt {
    /// The type of the spawn extension builder.
    type Spawner<'a>
    where
        Self: 'a;

    /// Get a spawn extension builder that provides the ability to:
    /// - Listen to stdout and/or stderr line by line in real-time via callbacks.
    fn spawn_builder(&mut self) -> Self::Spawner<'_>;
}

impl CmdSpawnExt for std::process::Command {
    type Spawner<'a> = CmdSpawnBuilderSync<'a>;

    fn spawn_builder(&mut self) -> Self::Spawner<'_> {
        CmdSpawnBuilderSync {
            command: self,
            on_stdout: None,
            on_stderr: None,
        }
    }
}

impl CmdSpawnExt for tokio::process::Command {
    type Spawner<'a> = CmdSpawnBuilderAsync<'a>;

    fn spawn_builder(&mut self) -> Self::Spawner<'_> {
        CmdSpawnBuilderAsync {
            command: self,
            on_stdout: None,
            on_stderr: None,
        }
    }
}

/// Synchronous command spawn extension builder. For [`std::process::Command`].
pub struct CmdSpawnBuilderSync<'a> {
    command: &'a mut std::process::Command,
    on_stdout: Option<Box<dyn Fn(String) + Sync + Send + 'static>>,
    on_stderr: Option<Arc<Box<dyn Fn(String) + Sync + Send + 'static>>>,
}

impl<'a> CmdSpawnBuilderSync<'a> {
    /// Set a callback to be called for each line of stdout.
    pub fn on_stdout(mut self, on_stdout: impl Fn(String) + Sync + Send + 'static) -> Self {
        self.on_stdout = Some(Box::new(on_stdout));
        self.command.stdout(Stdio::piped());
        self
    }

    /// Set a callback to be called for each line of stderr.
    pub fn on_stderr(mut self, on_stderr: impl Fn(String) + Sync + Send + 'static) -> Self {
        self.on_stderr = Some(Arc::new(Box::new(on_stderr)));
        self.command.stderr(Stdio::piped());
        self
    }

    /// Spawn the command.
    pub fn spawn(self) -> std::io::Result<std::process::Child> {
        let mut child = self.command.spawn()?;

        use std::io::BufRead;

        // Capture and print stdout in a separate thread
        if let Some(on_stdout) = self.on_stdout {
            let on_stderr = self.on_stderr.as_ref().map(|on_stderr| on_stderr.clone());
            if let Some(stdout) = child.stdout.take() {
                let stdout_reader = std::io::BufReader::new(stdout);
                std::thread::spawn(move || {
                    for line in stdout_reader.lines() {
                        match line {
                            Ok(line) => on_stdout(line),
                            Err(e) => {
                                let msg = format!("Error reading stdout: {:?}", e);
                                if let Some(on_stderr) = on_stderr.as_ref() {
                                    on_stderr(msg);
                                } else {
                                    on_stdout(msg);
                                }
                            }
                        }
                    }
                });
            }
        }

        // Capture and print stderr in a separate thread
        if let Some(on_stderr) = self.on_stderr {
            if let Some(stderr) = child.stderr.take() {
                let stderr_reader = std::io::BufReader::new(stderr);
                std::thread::spawn(move || {
                    for line in stderr_reader.lines() {
                        match line {
                            Ok(line) => on_stderr(line),
                            Err(e) => on_stderr(format!("Error reading stderr: {:?}", e)),
                        }
                    }
                });
            }
        }

        Ok(child)
    }
}

/// Asynchronous command spawn extension builder. For [`tokio::process::Command`].
pub struct CmdSpawnBuilderAsync<'a> {
    command: &'a mut tokio::process::Command,
    on_stdout: Option<Box<dyn Fn(String) -> BoxFuture<'static, ()> + Send + 'static>>,
    on_stderr: Option<Box<dyn Fn(String) -> BoxFuture<'static, ()> + Send + 'static>>,
}

impl<'a> CmdSpawnBuilderAsync<'a> {
    /// Set a callback to be called for each line of stdout.
    pub fn on_stdout<Fut: Future<Output = ()> + Send + 'static>(
        mut self,
        on_stdout: impl Fn(String) -> Fut + Send + 'static,
    ) -> Self {
        self.on_stdout = Some(Box::new(move |s| Box::pin(on_stdout(s))));
        self.command.stdout(Stdio::piped());
        self
    }

    /// Set a callback to be called for each line of stderr.
    pub fn on_stderr<Fut: Future<Output = ()> + Send + 'static>(
        mut self,
        on_stderr: impl Fn(String) -> Fut + Send + 'static,
    ) -> Self {
        self.on_stderr = Some(Box::new(move |s| Box::pin(on_stderr(s))));
        self.command.stderr(Stdio::piped());
        self
    }

    /// Spawn the command.
    pub fn spawn(self) -> std::io::Result<tokio::process::Child> {
        use tokio::io::AsyncBufReadExt;

        let mut child = self.command.spawn()?;

        // Capture and print stdout in a separate thread
        if let Some(on_stdout) = self.on_stdout {
            if let Some(stdout) = child.stdout.take() {
                let stdout_reader = tokio::io::BufReader::new(stdout);
                tokio::spawn(async move {
                    let mut lines = stdout_reader.lines();
                    loop {
                        match lines.next_line().await {
                            Ok(v) => match v {
                                Some(line) => on_stdout(line).await,
                                None => break,
                            },
                            Err(e) => {
                                on_stdout(format!("Error reading stdout: {:?}", e)).await;
                            }
                        }
                    }
                });
            }
        }

        // Capture and print stderr in a separate thread
        if let Some(on_stderr) = self.on_stderr {
            if let Some(stderr) = child.stderr.take() {
                let stderr_reader = tokio::io::BufReader::new(stderr);
                tokio::spawn(async move {
                    let mut lines = stderr_reader.lines();
                    loop {
                        match lines.next_line().await {
                            Ok(v) => match v {
                                Some(line) => on_stderr(line).await,
                                None => break,
                            },
                            Err(e) => on_stderr(format!("Error reading stderr: {:?}", e)).await,
                        }
                    }
                });
            }
        }

        Ok(child)
    }
}

// TESTING: implicitly tested during log tests which use it to extract logs I think.
