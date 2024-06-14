/// Prettify bytes into a string for a user using the 1000 base.
pub fn bytes_to_pretty_1000(bytes: u64) -> String {
    static UNITS: [&str; 9] = ["B", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
    static BASE: f64 = 1000.0;
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= BASE {
        size /= BASE;
        unit += 1;
    }
    format!("{:.1}{}", size, UNITS[unit])
}

/// Prettify bytes into a string for a user using the 1024 base.
pub fn bytes_to_pretty_1024(bytes: u64) -> String {
    static UNITS: [&str; 9] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB"];
    static BASE: f64 = 1024.0;
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= BASE {
        size /= BASE;
        unit += 1;
    }
    format!("{:.1}{}", size, UNITS[unit])
}

/// Convert bytes to megabits per second.
pub fn bytes_to_mbps(bytes: u64) -> f64 {
    bytes as f64 / 1000.0 / 1000.0 * 8.0
}
