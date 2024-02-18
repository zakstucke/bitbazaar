use colored::Colorize;
use tracing_core::Subscriber;
use tracing_subscriber::{
    fmt::{format::Writer, FmtContext, FormatEvent, FormatFields},
    registry::LookupSpan,
};

pub struct CustEventFormatter<
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
    T: FormatEvent<S, N>,
> {
    inner: T,
    _marker: std::marker::PhantomData<(S, N)>,
}

impl<S, N, T> CustEventFormatter<S, N, T>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
    T: FormatEvent<S, N>,
{
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<S, N, T> FormatEvent<S, N> for CustEventFormatter<S, N, T>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
    T: FormatEvent<S, N>,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let exception_fields = [
            "exception.message",
            "exception.type",
            "exception.stacktrace",
        ];
        let meta = event.metadata();

        let mut is_exception = false;
        for field in meta.fields() {
            if exception_fields.contains(&field.name()) {
                is_exception = true;
                break;
            }
        }

        if is_exception {
            let mut visitor = ExceptionEventVisitor::default();
            event.record(&mut visitor);

            let indent = 6;
            writeln!(writer, "{}", "ERROR: ".red())?;

            let msg = visitor.into_msg();
            // Write each line and indent it by 7 to match the ERROR: prefix
            for line in msg.lines() {
                writeln!(writer, "{:indent$}{}", "", line.red())?;
            }

            Ok(())
        } else {
            self.inner.format_event(ctx, writer, event)
        }
    }
}

#[derive(Default)]
struct ExceptionEventVisitor {
    message: Option<String>,
    typ: Option<String>,
    stacktrace: Option<String>,
}

impl ExceptionEventVisitor {
    fn into_msg(self) -> String {
        let mut msg = String::new();
        if let Some(stacktrace) = self.stacktrace {
            msg.push_str(clean_string(&stacktrace));
            msg.push('\n');
        }
        if let Some(typ) = self.typ {
            if let Some(message) = self.message {
                msg.push_str(&format!(
                    "{}: {}\n",
                    clean_string(&typ),
                    clean_string(&message)
                ));
            } else {
                msg.push_str(clean_string(&typ));
                msg.push('\n');
            }
        } else if let Some(message) = self.message {
            msg.push_str(clean_string(&message));
            msg.push('\n');
        }
        msg
    }
}

#[inline]
/// Weirdly they seem to come in with quotes around them, this simple removes them.
/// In a sep func to allow extending if needed.
fn clean_string(s: &str) -> &str {
    s.trim_matches('"')
}

impl tracing::field::Visit for ExceptionEventVisitor {
    fn record_str(&mut self, field: &tracing_core::Field, value: &str) {
        match field.name() {
            "exception.message" => self.message = Some(value.to_string()),
            "exception.type" => self.typ = Some(value.to_string()),
            "exception.stacktrace" => self.stacktrace = Some(value.to_string()),
            _ => {}
        }
    }

    /// NOTE: record_str() is the one that's actually used, this would escape newlines etc.
    /// But keeping as the trait requires it and just in case for some reason one of these isn't a string.
    fn record_debug(&mut self, field: &tracing_core::Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "exception.message" => self.message = Some(format!("{:?}", value)),
            "exception.type" => self.typ = Some(format!("{:?}", value)),
            "exception.stacktrace" => self.stacktrace = Some(format!("{:?}", value)),
            _ => {}
        }
    }
}
