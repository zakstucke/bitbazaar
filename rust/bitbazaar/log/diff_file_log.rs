use tracing::debug;

/// Allows testing logs coming from different files.
pub fn diff_file_log(log: &str) {
    debug!("{}", log)
}
