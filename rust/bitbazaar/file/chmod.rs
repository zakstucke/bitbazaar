use std::path::Path;

/// Chmod an open file, noop on Windows.
///
/// ENTER AS HEX!
///
/// E.g. `0o755` rather than `755`.
pub fn chmod_sync(mode: u32, filepath: &Path) -> Result<(), std::io::Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(filepath, std::fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}

/// Good default, chmod an open file to be executable by all, writeable by owner.
pub fn chmod_executable_sync(filepath: &Path) -> Result<(), std::io::Error> {
    chmod_sync(0o755, filepath)
}

/// Chmod an open file, noop on Windows.
///
/// ENTER AS HEX!
///
/// E.g. `0o755` rather than `755`.
pub async fn chmod_async(mode: u32, filepath: &Path) -> Result<(), std::io::Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&filepath, std::fs::Permissions::from_mode(mode)).await?;
    }
    Ok(())
}

/// Good default (755).
///
/// chmod an open file to be executable by all, writeable by owner.
pub async fn chmod_executable_async(filepath: &Path) -> Result<(), std::io::Error> {
    chmod_async(0o755, filepath).await
}
