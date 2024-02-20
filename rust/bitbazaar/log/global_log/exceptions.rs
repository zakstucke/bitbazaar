// Inner for record_exception to allow specifying type internally.
pub fn record_exception_inner(
    message: impl Into<String>,
    stacktrace: impl Into<String>,
    typ: &str,
) {
    tracing::event!(
        tracing::Level::ERROR,
        name = "exception", // Must be named this for observers to recognise it as an exception
        exception.message = message.into(),
        exception.stacktrace = stacktrace.into(),
        "exception.type" = typ
    );
}

/// Setup the program to automatically log panics as an error event on the current span.
/// Internally makes sure it only runs once.
pub fn auto_trace_panics() {
    use std::sync::Once;
    static SET_HOOK: Once = Once::new();
    SET_HOOK.call_once(|| {
        // If wasm, know the default browser panic hook is dogshite, so just replace so it doesn't show:
        #[cfg(target_arch = "wasm32")]
        {
            std::panic::set_hook(Box::new(panic_hook));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let prev_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |panic_info| {
                panic_hook(panic_info);
                prev_hook(panic_info);
            }));
        }
    });
}

fn panic_hook(panic_info: &std::panic::PanicInfo) {
    let payload = panic_info.payload();

    #[allow(clippy::manual_map)]
    let payload = if let Some(s) = payload.downcast_ref::<&str>() {
        Some(&**s)
    } else if let Some(s) = payload.downcast_ref::<String>() {
        Some(s.as_str())
    } else {
        None
    };

    let location = panic_info.location().map(|l| l.to_string());
    super::exceptions::record_exception_inner(
        payload.unwrap_or("Panic missing message."),
        location.unwrap_or_else(|| "Panic missing location.".to_string()),
        "Panic",
    );
}
