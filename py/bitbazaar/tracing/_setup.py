import logging
import os
import pathlib
import typing as tp

from opentelemetry import trace
from opentelemetry._logs import set_logger_provider
from opentelemetry.exporter.otlp.proto.grpc.trace_exporter import OTLPSpanExporter
from opentelemetry.sdk import resources
from opentelemetry.sdk._logs import LoggerProvider, LoggingHandler
from opentelemetry.sdk._logs.export import BatchLogRecordProcessor, SimpleLogRecordProcessor
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import (
    BatchSpanProcessor,
)

from ._filterables import (
    FilterableConsoleLogExporter,
    FilterableFileLogExporter,
    FilterableOTLPLogExporter,
)
from ._formatting import console_log_formatter


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


class FileSink(tp.TypedDict):
    # Will log this level and up:
    from_level: int
    # The file to write to:
    logpath: pathlib.Path
    # Will start new file when this size is reached: (prev will be renamed to .1, .2, etc.)
    max_bytes: int
    # Will keep this many backups:
    max_backups: int


class Args(tp.TypedDict):
    service_name: str
    console: tp.NotRequired[ConsoleSink]
    otlp: tp.NotRequired[OLTPSink]
    file: tp.NotRequired[FileSink]


def prepare_provider(args: Args) -> TracerProvider:
    console = args.get("console", None)
    otlp = args.get("otlp", None)
    file = args.get("file", None)

    resource = resources.Resource(
        attributes={
            resources.SERVICE_NAME: args["service_name"],
            resources.SERVICE_INSTANCE_ID: os.uname().nodename,
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

    # Set the root logging level to the lowest needed:
    logging.basicConfig(level=lowest_lvl_filter, force=True)

    # Clear any existing handlers, so ours is the only one:
    logging.getLogger().handlers.clear()

    # Add the handler for oltp: (console/backend/file will be handled through this single handler)
    logging.getLogger().addHandler(log_handler)

    if otlp is not None:
        log_provider.add_log_record_processor(
            BatchLogRecordProcessor(
                FilterableOTLPLogExporter(endpoint=otlp["url"], headers=otlp["headers"]).from_level(
                    otlp["from_level"]
                )
            )
        )
        trace_provider.add_span_processor(
            BatchSpanProcessor(OTLPSpanExporter(endpoint=otlp["url"], headers=otlp["headers"]))
        )

    if console is not None:
        # No spans in console, no easy way of connecting them up, used in dev where you know whats going on anyway so shouldn't cause too many issues.
        log_provider.add_log_record_processor(
            SimpleLogRecordProcessor(
                FilterableConsoleLogExporter(formatter=console_log_formatter).from_level(
                    console["from_level"]
                )
            )
        )

    if file is not None:
        # No spans in files, no easy way of connecting them up,
        # sid included though so e.g. if looking for a debug log (where only info and up in oltp) you can find it using the sid.
        log_provider.add_log_record_processor(
            SimpleLogRecordProcessor(
                FilterableFileLogExporter(
                    filepath=file["logpath"],
                    max_bytes=file["max_bytes"],
                    max_backups=file["max_backups"],
                ).from_level(file["from_level"])
            )
        )

    trace.set_tracer_provider(trace_provider)
    set_logger_provider(log_provider)

    return trace_provider
