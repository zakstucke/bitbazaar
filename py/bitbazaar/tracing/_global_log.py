import logging

from opentelemetry import trace
from opentelemetry.context import Context
from opentelemetry.sdk._logs import LoggerProvider
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.trace import SpanKind, _Links
from opentelemetry.util.types import Attributes

from bitbazaar import utils

from ._file_handler import CustomRotatingFileHandler
from ._setup import ConsoleSink, FileSink, OLTPSink, prepare_providers


class GlobalLog:
    tracer_provider: TracerProvider
    logger_provider: LoggerProvider
    tracer: trace.Tracer
    file_handler: CustomRotatingFileHandler | None

    def __init__(
        self,
        service_name: str,
        console: ConsoleSink | None = None,
        otlp: OLTPSink | None = None,
        file: FileSink | None = None,
    ):
        """Initialize tracing for a project.

        NOTE: only the otlp sink logs spans, but file logger will include sid in logs, to allow finding them from oltp (e.g. debug in file, but only info up in oltp.)
        """
        self.tracer_provider, self.logger_provider, self.file_handler = prepare_providers(
            {
                "service_name": service_name,
                "console": console,
                "otlp": otlp,
                "file": file,
            }
        )
        self.tracer = trace.get_tracer("GlobalLog")

    @utils.copy_sig(logging.debug)
    def debug(self, *args, **kwargs):  # type: ignore
        logging.debug(*args, **kwargs)

    @utils.copy_sig(logging.info)
    def info(self, *args, **kwargs):  # type: ignore
        logging.info(*args, **kwargs)

    @utils.copy_sig(logging.warning)
    def warn(self, *args, **kwargs):  # type: ignore
        logging.warning(*args, **kwargs)

    @utils.copy_sig(logging.error)
    def error(self, *args, **kwargs):  # type: ignore
        logging.error(*args, **kwargs)

    @utils.copy_sig(logging.critical)
    def crit(self, *args, **kwargs):  # type: ignore
        logging.critical(*args, **kwargs)

    # Can't copy sig because different self types, effectively just copied full interface to not lose information.
    def span(
        self,
        name: str,
        context: Context | None = None,
        kind: SpanKind = SpanKind.INTERNAL,
        attributes: Attributes = None,
        links: _Links = None,
        start_time: int | None = None,
        record_exception: bool = True,
        set_status_on_exception: bool = True,
        end_on_exit: bool = True,
    ):
        """Context manager for creating a new span and set it as the current span in this tracer's context.

        Exiting the context manager will call the span's end method,
        as well as return the current span to its previous value by
        returning to the previous context.

        Example::

            with tracer.start_as_current_span("one") as parent:
                parent.add_event("parent's event")
                with tracer.start_as_current_span("two") as child:
                    child.add_event("child's event")
                    trace.get_current_span()  # returns child
                trace.get_current_span()      # returns parent
            trace.get_current_span()          # returns previously active span

        This is a convenience method for creating spans attached to the
        tracer's context. Applications that need more control over the span
        lifetime should use :meth:`start_span` instead. For example::

            with tracer.start_as_current_span(name) as span:
                do_work()

        is equivalent to::

            span = tracer.start_span(name)
            with opentelemetry.trace.use_span(span, end_on_exit=True):
                do_work()

        This can also be used as a decorator::

            @tracer.start_as_current_span("name")
            def function():
                ...

            function()

        Args:
            name: The name of the span to be created.
            context: An optional Context containing the span's parent. Defaults to the
                global context.
            kind: The span's kind (relationship to parent). Note that is
                meaningful even if there is no parent.
            attributes: The span's attributes.
            links: Links span to other spans
            start_time: Sets the start time of a span
            record_exception: Whether to record any exceptions raised within the
                context as error event on the span.
            set_status_on_exception: Only relevant if the returned span is used
                in a with/context manager. Defines whether the span status will
                be automatically set to ERROR when an uncaught exception is
                raised in the span with block. The span status won't be set by
                this mechanism if it was previously set manually.
            end_on_exit: Whether to end the span automatically when leaving the
                context manager.

        Yields:
            The newly-created span.
        """
        return self.tracer.start_as_current_span(
            name,
            context,
            kind,
            attributes,
            links,
            start_time,
            record_exception,
            set_status_on_exception,
            end_on_exit,
        )

    def flush(self) -> None:
        """Force all logs/spans through, useful when testing."""
        self.tracer_provider.force_flush()
        self.logger_provider.force_flush()
        if self.file_handler:
            self.file_handler.flush()

    def shutdown(self) -> None:
        """Shuts/closes everything down. Happens automatically at end of program anyway."""
        self.tracer_provider.shutdown()
        self.logger_provider.shutdown()
        if self.file_handler:
            self.file_handler.close()

    # TODO in some format, maybe in a generic creator for fastapi/django/celery, but probably outside the tracing module interacting with the tracer_provider.
    # def instrument_fastapi(self, app: "FastAPI") -> None:
    #     """Instrument fastapi with automatic tracing. 'fastapi' extra pkg feature must be installed."""
    #     try:
    #         from opentelemetry.instrumentation.fastapi import FastAPIInstrumentor
    #     except ImportError as e:
    #         raise ImportError(
    #             "To use this method, you must have the 'fastapi' extra pkg feature installed."
    #         ) from e

    #     FastAPIInstrumentor.instrument_app(app, tracer_provider=self.tracer_provider)

    # def instrument_django(self) -> None:
    #     """Instrument django with automatic tracing. 'django' extra pkg feature must be installed."""
    #     try:
    #         from opentelemetry.instrumentation.django import DjangoInstrumentor
    #     except ImportError as e:
    #         raise ImportError(
    #             "To use this method, you must have the 'django' extra pkg feature installed."
    #         ) from e

    #     # Make sure DJANGO_SETTINGS_MODULE env var is set:
    #     if not os.environ.get("DJANGO_SETTINGS_MODULE"):
    #         raise ValueError(
    #             "'DJANGO_SETTINGS_MODULE' env var must already be set to instrument django."
    #         )

    #     DjangoInstrumentor().instrument()

    # def instrument_celery(self) -> None:
    #     """Instrument celery with automatic tracing. 'celery' extra pkg feature must be installed."""
    #     try:
    #         from opentelemetry.instrumentation.celery import CeleryInstrumentor
    #     except ImportError as e:
    #         raise ImportError(
    #             "To use this method, you must have the 'celery' extra pkg feature installed."
    #         ) from e
    #     CeleryInstrumentor().instrument()
