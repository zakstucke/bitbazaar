// TODO: copied latest version of opentelemetry-appender-tracing as need the new feature 'experimental_metadata_attributes' to get access to file, line, module_path, target etc.
// once they release a new version (>0.2), can use that, because of the complex interop with the different crates, this was the easiest way to get latest version.
// TODO: note also second hotfix in here from https://github.com/open-telemetry/opentelemetry-rust/pull/1394/files,
// don't update until the new version fixing tracing span!()/instrument propagation too.

use std::borrow::Cow;

use opentelemetry::{
    logs::{AnyValue, LogRecord, Logger, LoggerProvider, Severity},
    Key,
};
use tracing_core::{Level, Metadata, Subscriber};
use tracing_log::NormalizeEvent;
use tracing_subscriber::{registry::LookupSpan, Layer};

const INSTRUMENTATION_LIBRARY_NAME: &str = "opentelemetry-appender-tracing";

/// Visitor to record the fields from the event record.
#[derive(Default)]
struct EventVisitor {
    log_record_attributes: Vec<(Key, AnyValue)>,
    log_record_body: Option<AnyValue>,
}

/// Logs from the log crate have duplicated attributes that we removed here.
fn is_duplicated_metadata(field: &'static str) -> bool {
    field
        .strip_prefix("log.")
        .map(|remainder| matches!(remainder, "file" | "line" | "module_path" | "target"))
        .unwrap_or(false)
}

fn get_filename(filepath: &str) -> &str {
    if let Some((_, filename)) = filepath.rsplit_once('/') {
        return filename;
    }
    if let Some((_, filename)) = filepath.rsplit_once('\\') {
        return filename;
    }
    filepath
}

impl EventVisitor {
    fn visit_metadata(&mut self, meta: &Metadata) {
        self.log_record_attributes
            .push(("name".into(), meta.name().into()));

        self.visit_experimental_metadata(meta);
    }

    fn visit_experimental_metadata(&mut self, meta: &Metadata) {
        self.log_record_attributes
            .push(("log.target".into(), meta.target().to_owned().into()));

        if let Some(module_path) = meta.module_path() {
            self.log_record_attributes
                .push(("code.namespace".into(), module_path.to_owned().into()));
        }

        if let Some(filepath) = meta.file() {
            self.log_record_attributes
                .push(("code.filepath".into(), filepath.to_owned().into()));
            self.log_record_attributes.push((
                "code.filename".into(),
                get_filename(filepath).to_owned().into(),
            ));
        }

        if let Some(line) = meta.line() {
            self.log_record_attributes
                .push(("code.lineno".into(), line.into()));
        }
    }

    fn push_to_otel_log_record(self, log_record: &mut LogRecord) {
        log_record.body = self.log_record_body;
        log_record.attributes = Some(self.log_record_attributes);
    }
}

impl tracing::field::Visit for EventVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if is_duplicated_metadata(field.name()) {
            return;
        }
        if field.name() == "message" {
            self.log_record_body = Some(format!("{value:?}").into());
        } else {
            self.log_record_attributes
                .push((field.name().into(), format!("{value:?}").into()));
        }
    }

    fn record_str(&mut self, field: &tracing_core::Field, value: &str) {
        if is_duplicated_metadata(field.name()) {
            return;
        }
        self.log_record_attributes
            .push((field.name().into(), value.to_owned().into()));
    }

    fn record_bool(&mut self, field: &tracing_core::Field, value: bool) {
        self.log_record_attributes
            .push((field.name().into(), value.into()));
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.log_record_attributes
            .push((field.name().into(), value.into()));
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        if is_duplicated_metadata(field.name()) {
            return;
        }
        self.log_record_attributes
            .push((field.name().into(), value.into()));
    }

    // TODO: Remaining field types from AnyValue : Bytes, ListAny, Boolean
}

pub struct OpenTelemetryTracingBridge<P, L>
where
    P: LoggerProvider<Logger = L> + Send + Sync,
    L: Logger + Send + Sync,
{
    logger: L,
    _phantom: std::marker::PhantomData<P>, // P is not used.
}

impl<P, L> OpenTelemetryTracingBridge<P, L>
where
    P: LoggerProvider<Logger = L> + Send + Sync,
    L: Logger + Send + Sync,
{
    pub fn new(provider: &P) -> Self {
        OpenTelemetryTracingBridge {
            logger: provider.versioned_logger(
                INSTRUMENTATION_LIBRARY_NAME,
                Some(Cow::Borrowed(env!("CARGO_PKG_VERSION"))),
                None,
                None,
            ),
            _phantom: Default::default(),
        }
    }
}

impl<S, P, L> Layer<S> for OpenTelemetryTracingBridge<P, L>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    P: LoggerProvider<Logger = L> + Send + Sync + 'static,
    L: Logger + Send + Sync + 'static,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let normalized_meta = event.normalized_metadata();
        let meta = normalized_meta.as_ref().unwrap_or_else(|| event.metadata());

        let mut log_record: LogRecord = LogRecord::default();
        log_record.severity_number = Some(severity_of_level(meta.level()));
        log_record.severity_text = Some(meta.level().to_string().into());

        set_trace_context(&mut log_record, &_ctx);

        // Not populating ObservedTimestamp, instead relying on OpenTelemetry
        // API to populate it with current time.

        let mut visitor = EventVisitor::default();
        visitor.visit_metadata(meta);
        // Visit fields.
        event.record(&mut visitor);
        visitor.push_to_otel_log_record(&mut log_record);

        self.logger.emit(log_record);
    }

    // #[cfg(feature = "logs_level_enabled")]
    // fn event_enabled(
    //     &self,
    //     _event: &tracing_core::Event<'_>,
    //     _ctx: tracing_subscriber::ot_tracing_bridge::Context<'_, S>,
    // ) -> bool {
    //     let severity = severity_of_level(_event.metadata().level());
    //     self.logger
    //         .event_enabled(severity, _event.metadata().target())
    // }
}

const fn severity_of_level(level: &Level) -> Severity {
    match *level {
        Level::TRACE => Severity::Trace,
        Level::DEBUG => Severity::Debug,
        Level::INFO => Severity::Info,
        Level::WARN => Severity::Warn,
        Level::ERROR => Severity::Error,
    }
}

// TODO added by me from https://github.com/open-telemetry/opentelemetry-rust/pull/1394/files to fix the
fn set_trace_context<S>(log_record: &mut LogRecord, ctx: &tracing_subscriber::layer::Context<'_, S>)
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    use opentelemetry::{
        logs::TraceContext,
        trace::{SpanContext, TraceFlags, TraceState},
    };

    if let Some((trace_id, span_id)) = ctx.lookup_current().and_then(|span| {
        span.extensions()
            .get::<tracing_opentelemetry::OtelData>()
            .and_then(|ext| ext.builder.trace_id.zip(ext.builder.span_id))
    }) {
        log_record.trace_context = Some(TraceContext::from(&SpanContext::new(
            trace_id,
            span_id,
            TraceFlags::default(),
            false,
            TraceState::default(),
        )));
    }
}
