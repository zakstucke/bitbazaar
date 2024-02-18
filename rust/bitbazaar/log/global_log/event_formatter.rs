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
            if let Some(stacktrace) = visitor.stacktrace.as_ref() {
                writeln!(writer, "{}", clean_string(stacktrace))?;
            }
            if let Some(typ) = visitor.typ.as_ref() {
                if let Some(message) = visitor.message.as_ref() {
                    writeln!(writer, "{}: {}", clean_string(typ), clean_string(message))?;
                } else {
                    writeln!(writer, "{}", clean_string(typ))?;
                }
            } else if let Some(message) = visitor.message.as_ref() {
                writeln!(writer, "{}", clean_string(message))?;
            }
            Ok(())
        } else {
            self.inner.format_event(ctx, writer, event)
        }
    }
}

#[inline]
/// Weirdly they seem to come in with quotes around them, this simple removes them.
/// In a sep func to allow extending if needed.
fn clean_string(s: &str) -> &str {
    s.trim_matches('"')
}

#[derive(Default)]
struct ExceptionEventVisitor {
    message: Option<String>,
    typ: Option<String>,
    stacktrace: Option<String>,
}

impl tracing::field::Visit for ExceptionEventVisitor {
    fn record_debug(&mut self, field: &tracing_core::Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "exception.message" => self.message = Some(format!("{:?}", value)),
            "exception.type" => self.typ = Some(format!("{:?}", value)),
            "exception.stacktrace" => self.stacktrace = Some(format!("{:?}", value)),
            _ => {}
        }
    }
}
