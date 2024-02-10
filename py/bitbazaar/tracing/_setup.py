import logging
import platform
import typing as tp

import opentelemetry._logs._internal
import opentelemetry.trace
from opentelemetry import trace
from opentelemetry._logs import set_logger_provider
from opentelemetry.sdk import resources
from opentelemetry.sdk._logs import LoggerProvider, LoggingHandler
from opentelemetry.sdk._logs.export import BatchLogRecordProcessor
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import BatchSpanProcessor

import bitbazaar._testing

from ._exporters import (
    CustConsoleLogExporter,
    CustConsoleSpanExporter,
    CustFileLogExporter,
    CustFileSpanExporter,
    CustOTLPLogExporter,
    CustOTLPSpanExporter,
)
from ._file_handler import CustomRotatingFileHandler
from ._formatting import console_log_formatter, console_span_formatter

if tp.TYPE_CHECKING:  # pragma: no cover
    from _typeshed import StrPath


class OLTPSink(tp.TypedDict):
    # Will log this level and up:
    from_level: int
    # The open telemetry collector provider url endpoint:
    url: str
    # Headers to send with requests to the provider:
    headers: dict[str, str]


class ConsoleSink(tp.TypedDict):
    # Will log this level and up:
    from_level: int
    # Show spans in console, if true will be shown dimmed as logs are usually more important on the console.
    spans: bool


class FileSink(tp.TypedDict):
    # Will log this level and up:
    from_level: int
    # The file to write to:
    logpath: "StrPath"
    # Will start new file when this size is reached: (prev will be renamed to .1, .2, etc.)
    max_bytes: int
    # Will keep this many backups:
    max_backups: int


class Args(tp.TypedDict):
    service_name: str
    console: ConsoleSink | None
    otlp: OLTPSink | None
    file: FileSink | None


def prepare_providers(
    args: Args,
) -> tuple[TracerProvider, LoggerProvider, CustomRotatingFileHandler | None]:
    console = args["console"]
    otlp = args["otlp"]
    file = args["file"]

    resource = resources.Resource(
        attributes={
            resources.SERVICE_NAME: args["service_name"],
            resources.SERVICE_INSTANCE_ID: platform.uname().node,  # Instead of os.uname().nodename to work with windows as well.
        }
    )

    # Get all the active log level filters:
    all_lvl_filters = [
        val
        for val in [
            console["from_level"] if console is not None else None,
            otlp["from_level"] if otlp is not None else None,
            file["from_level"] if file is not None else None,
        ]
        if val is not None
    ]
    lowest_lvl_filter = min(all_lvl_filters) if all_lvl_filters else logging.NOTSET

    trace_provider = TracerProvider(resource=resource)
    log_provider = LoggerProvider(resource=resource)
    log_handler = LoggingHandler(logger_provider=log_provider, level=logging.DEBUG)
    file_handler: CustomRotatingFileHandler | None = None

    # Set the root logging level to the lowest needed:
    logging.basicConfig(level=lowest_lvl_filter, force=True)

    # Clear any existing handlers, so ours is the only one:
    logging.getLogger().handlers.clear()

    # Add the handler for oltp: (console/backend/file will be handled through this single handler)
    logging.getLogger().addHandler(log_handler)

    if otlp is not None:
        log_provider.add_log_record_processor(
            BatchLogRecordProcessor(
                CustOTLPLogExporter(endpoint=otlp["url"], headers=otlp["headers"]).from_level(
                    otlp["from_level"]
                )
            )
        )
        trace_provider.add_span_processor(
            BatchSpanProcessor(CustOTLPSpanExporter(endpoint=otlp["url"], headers=otlp["headers"]))
        )

    if console is not None:
        log_provider.add_log_record_processor(
            BatchLogRecordProcessor(
                CustConsoleLogExporter(
                    formatter=lambda record: console_log_formatter(record, console["spans"])
                ).from_level(console["from_level"])
            )
        )
        if console["spans"]:
            trace_provider.add_span_processor(
                BatchSpanProcessor(CustConsoleSpanExporter(formatter=console_span_formatter))
            )

    if file is not None:
        file_handler = CustomRotatingFileHandler(
            file["logpath"], maxBytes=file["max_bytes"], backupCount=file["max_backups"]
        )
        log_provider.add_log_record_processor(
            BatchLogRecordProcessor(
                CustFileLogExporter(handler=file_handler).from_level(file["from_level"])
            )
        )
        trace_provider.add_span_processor(
            BatchSpanProcessor(CustFileSpanExporter(handler=file_handler))
        )

    # Allow replacing if testing:
    if bitbazaar._testing.IS_TEST and opentelemetry.trace._TRACER_PROVIDER is not None:
        opentelemetry.trace._TRACER_PROVIDER = trace_provider
    else:
        trace.set_tracer_provider(trace_provider)

    # Allow replacing if testing:
    if bitbazaar._testing.IS_TEST and opentelemetry._logs._internal._LOGGER_PROVIDER is not None:
        opentelemetry._logs._internal._LOGGER_PROVIDER = log_provider
    else:
        set_logger_provider(log_provider)

    return trace_provider, log_provider, file_handler
