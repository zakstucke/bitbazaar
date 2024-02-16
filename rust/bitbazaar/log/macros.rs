use once_cell::sync::Lazy;
use parking_lot::Mutex;

#[allow(dead_code)]
pub static SENT_WARNING_IDS: Lazy<Mutex<Vec<&'static str>>> = Lazy::new(Mutex::default);

/// Warn a user once, with uniqueness determined by the given ID.
#[macro_export]
macro_rules! warn_user_once_by_id {
    ($id:expr, $($arg:tt)*) => {
        use tracing::warn;

        if let Ok(mut states) = $crate::logging::SENT_WARNING_IDS.lock() {
            if !states.contains(&$id) {
                let message = format!("{}", format_args!($($arg)*));
                warn!("{}", message);
                states.push($id);
            }
        }
    };
}

/// Warn a user once, with uniqueness determined by the calling location itself.
#[macro_export]
macro_rules! warn_user_once {
    ($($arg:tt)*) => {
        use tracing::warn;

        static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !WARNED.swap(true, std::sync::atomic::Ordering::SeqCst) {
            let message = format!("{}", format_args!($($arg)*));
            warn!("{}", message);
        }
    };
}
