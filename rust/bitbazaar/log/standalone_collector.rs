use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;

use crate::log::record_exception;
use crate::misc::{platform, setup_once, tarball_decompress};
use crate::prelude::*;

/// Standalone opentelemetry collector, using a unique free port.
/// Useful for testing.
/// Manages initial installation for you.
pub struct CollectorStandalone {
    child: tokio::process::Child,
    // So it doesn't get dropped before the collector's had time to read:
    _config_file: NamedTempFile,
}

impl CollectorStandalone {
    /// Start a standalone collector process on an unused port.
    /// This process will be killed on drop.
    ///
    /// Arguments:
    /// - config: the config file contents to pass to the collector.
    /// - on_stdout: what to do with each stdout line emitted by the process
    /// - on_stderr: what to do with each stderr line emitted by the process
    pub async fn new(
        config: &str,
        on_stdout: impl Fn(String) + Send + 'static + Clone,
        on_stderr: impl Fn(String) + Send + 'static + Clone,
    ) -> RResult<Self, AnyErr> {
        let mut config_file = NamedTempFile::new().change_context(AnyErr)?;
        config_file
            .write_all(config.as_bytes())
            .change_context(AnyErr)?;
        let config_filepath = config_file.path();

        static COLLECTOR_BINARY_NAME: &str = if cfg!(windows) {
            "collector.exe"
        } else {
            "collector"
        };
        static COLLECTOR_VERSION: &str = "0.106.1";

        async fn spawn_child(
            workspace_dir: PathBuf,
            config_filepath: &Path,
            on_stdout: impl Fn(String) + Send + 'static,
            on_stderr: impl Fn(String) + Send + 'static + Clone,
        ) -> RResult<tokio::process::Child, AnyErr> {
            tokio::process::Command::new(workspace_dir.join(COLLECTOR_BINARY_NAME))
                .arg("--config")
                .arg(config_filepath)
                .spawn_with_managed_std(on_stdout, on_stderr)
                .change_context(AnyErr)
        }

        let child = setup_once(
            "opentelemetry_collector",
            COLLECTOR_VERSION,
            true,
            {
                let on_stdout = on_stdout.clone();
                let on_stderr = on_stderr.clone();
                |workspace_dir| async move {
                    let os_type = match platform::os_type() {
                        platform::OsType::Windows => "windows",
                        platform::OsType::Linux => "linux",
                        platform::OsType::Macos => "darwin",
                        platform::OsType::Unknown => return Err(anyerr!("Unknown OS type.")),
                    };

                    let arch = match platform::architecture_type() {
                        platform::Arch::X64 => "amd64",
                        platform::Arch::Arm => "arm64",
                        platform::Arch::X32 => {
                            return Err(anyerr!("Unsupported architecture type: x32"))
                        }
                        platform::Arch::Other(arch) => {
                            return Err(anyerr!("Unknown architecture type: {}", arch))
                        }
                    };

                    let download_url = format!(
                        "https://github.com/open-telemetry/opentelemetry-collector-releases/\
                        releases/download/v{}/otelcol_{}_{}_{}.tar.gz",
                        COLLECTOR_VERSION, COLLECTOR_VERSION, os_type, arch
                    );

                    // Download using reqwest:
                    let response = reqwest::get(&download_url).await.change_context(AnyErr)?;
                    if response.status() != reqwest::StatusCode::OK {
                        return Err(anyerr!(
                            "Could not download collector binary. Url {} returned status code {}.",
                            download_url,
                            response.status()
                        ));
                    }

                    let downloaded_bin_name = if cfg!(windows) {
                        "otelcol.exe"
                    } else {
                        "otelcol"
                    };

                    let mut seen_paths = vec![];
                    let binary = tarball_decompress(
                        Cursor::new(response.bytes().await.change_context(AnyErr)?),
                        None,
                        |mut looper| {
                            let path = looper.value().path()?.to_string_lossy().to_string();
                            if path == downloaded_bin_name {
                                let mut buf = vec![];
                                looper
                                    .value_mut()
                                    .read_to_end(&mut buf)
                                    .change_context(AnyErr)?;
                                *looper.state_mut() = Some(buf);
                                looper.stop_early();
                            }
                            seen_paths.push(path);
                            Ok(looper)
                        },
                    )?
                    .ok_or_else(|| {
                        anyerr!(
                            "Could not find collector binary named \"{}\" in downloaded tarball. \
                    Available files: {:?}",
                            downloaded_bin_name,
                            seen_paths
                        )
                    })?;

                    // Save the binary
                    let filepath = workspace_dir.join(COLLECTOR_BINARY_NAME);
                    let mut file = tokio::fs::File::create(&filepath)
                        .await
                        .change_context(AnyErr)?;

                    file.write_all(&binary).await.change_context(AnyErr)?;

                    // TODONOW utility
                    // Execute permissions:
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        tokio::fs::set_permissions(
                            &filepath,
                            std::fs::Permissions::from_mode(0o755),
                        )
                        .await
                        .change_context(AnyErr)?;
                    }
                    // Before adding a small sleep, on macos I'd randomly get Malformed Mach-o file (os error 88) when instantly trying to run binary after above:
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                    let child =
                        spawn_child(workspace_dir, config_filepath, on_stdout, on_stderr).await?;

                    // It seems after initial setup, the binary takes a chunk more time to start up.
                    // Tests error when this is 700ms or less on my PC, so 1500 to be safe:
                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

                    Ok(child)
                }
            },
            |workspace_dir| spawn_child(workspace_dir, config_filepath, on_stdout, on_stderr),
        )
        .await?;

        Ok(Self {
            child,
            _config_file: config_file,
        })
    }

    /// Kill the server, will be automatically called when dropped.
    pub fn kill(mut self) {
        self.kill_inner()
    }

    fn kill_inner(&mut self) {
        match self.child.start_kill() {
            Ok(_) => {}
            Err(e) => record_exception("Could not kill child process.", format!("{:?}", e)),
        }
    }
}

impl Drop for CollectorStandalone {
    fn drop(&mut self) {
        self.kill_inner()
    }
}

/// TODONOW move somewhere else to generalise/expose and finalise name.
trait CmdSpawnWithManagedStd {
    type Child;

    fn spawn_with_managed_std(
        &mut self,
        on_stdout: impl Fn(String) + Send + 'static,
        on_stderr: impl Fn(String) + Send + 'static + Clone,
    ) -> std::io::Result<Self::Child>;
}

impl CmdSpawnWithManagedStd for std::process::Command {
    type Child = std::process::Child;

    fn spawn_with_managed_std(
        &mut self,
        on_stdout: impl Fn(String) + Send + 'static,
        on_stderr: impl Fn(String) + Send + 'static + Clone,
    ) -> std::io::Result<Self::Child> {
        let mut child = self.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;

        use std::io::BufRead;

        // Capture and print stdout in a separate thread
        if let Some(stdout) = child.stdout.take() {
            let on_stderr = on_stderr.clone();
            let stdout_reader = std::io::BufReader::new(stdout);
            std::thread::spawn(move || {
                for line in stdout_reader.lines() {
                    match line {
                        Ok(line) => on_stdout(line),
                        Err(e) => on_stderr(format!("Error reading stdout: {:?}", e)),
                    }
                }
            });
        }

        // Capture and print stderr in a separate thread
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

        Ok(child)
    }
}

impl CmdSpawnWithManagedStd for tokio::process::Command {
    type Child = tokio::process::Child;

    fn spawn_with_managed_std(
        &mut self,
        on_stdout: impl Fn(String) + Send + 'static,
        on_stderr: impl Fn(String) + Send + 'static + Clone,
    ) -> tokio::io::Result<Self::Child> {
        use tokio::io::AsyncBufReadExt;

        let mut child = self.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;

        // Capture and print stdout in a separate thread
        if let Some(stdout) = child.stdout.take() {
            let on_stderr = on_stderr.clone();
            let stdout_reader = tokio::io::BufReader::new(stdout);
            tokio::spawn(async move {
                let mut lines = stdout_reader.lines();
                loop {
                    match lines.next_line().await {
                        Ok(v) => match v {
                            Some(line) => on_stdout(line),
                            None => break,
                        },
                        Err(e) => on_stderr(format!("Error reading stdout: {:?}", e)),
                    }
                }
            });
        }

        // Capture and print stderr in a separate thread
        if let Some(stderr) = child.stderr.take() {
            let stderr_reader = tokio::io::BufReader::new(stderr);
            tokio::spawn(async move {
                let mut lines = stderr_reader.lines();
                loop {
                    match lines.next_line().await {
                        Ok(v) => match v {
                            Some(line) => on_stderr(line),
                            None => break,
                        },
                        Err(e) => on_stderr(format!("Error reading stderr: {:?}", e)),
                    }
                }
            });
        }

        Ok(child)
    }
}
