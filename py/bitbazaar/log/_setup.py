import logging
import platform
import sys
import typing as tp

import opentelemetry._logs._internal
import opentelemetry.trace
from opentelemetry import trace
from opentelemetry._logs import set_logger_provider
from opentelemetry.exporter.otlp.proto.grpc.metric_exporter import (
    OTLPMetricExporter as OTLPMetricExporterGRPC,
)
from opentelemetry.sdk import resources
from opentelemetry.sdk._logs import LoggerProvider, LoggingHandler
from opentelemetry.sdk._logs.export import BatchLogRecordProcessor
from opentelemetry.sdk.metrics import MeterProvider
from opentelemetry.sdk.metrics.export import (
    ConsoleMetricExporter,
    MetricReader,
    PeriodicExportingMetricReader,
)
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import BatchSpanProcessor

import bitbazaar._testing
from bitbazaar.misc import is_tcp_port_listening

from ._exporters import (
    CustConsoleLogExporter,
    CustConsoleSpanExporter,
    CustFileLogExporter,
    CustFileMetricExporter,
    CustFileSpanExporter,
    CustOTLPLogExporterGRPC,
    CustOTLPSpanExporterGRPC,
)
from ._file_handler import CustomRotatingFileHandler
from ._formatting import console_log_formatter, console_metric_formatter, console_span_formatter

if tp.TYPE_CHECKING:  # pragma: no cover
    from _typeshed import StrPath


class OLTPSink(tp.TypedDict):
    # Will log this level and up:
    from_level: int
    # The local port to speak to the local open telemetry collector on:
    port: int


class ConsoleSink(tp.TypedDict):
    # Will log this level and up:
    from_level: int
    # Show spans in console, if true will be shown dimmed as logs are usually more important on the console.
    spans: bool
    # Show metrics in console, if true will be shown dimmed as logs are usually more important on the console.
    metrics: bool
    # Optionally overwrite the writer from sys.stdout, useful to store during e.g. testing:
    writer: tp.NotRequired[tp.IO]


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
    service_version: str
    console: ConsoleSink | None
    otlp: OLTPSink | None
    file: FileSink | None


def prepare_providers(
    args: Args,
) -> tuple[MeterProvider, TracerProvider, LoggerProvider, CustomRotatingFileHandler | None]:
    console = args["console"]
    otlp = args["otlp"]
    file = args["file"]

    resource = resources.Resource(
        attributes={
            resources.SERVICE_NAME: args["service_name"],
            resources.SERVICE_VERSION: args["service_version"],
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

    metric_readers: list["MetricReader"] = []
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

    if otlp is not None:  # pragma: no cover (is covered but not in CI)
        if not is_tcp_port_listening("localhost", otlp["port"]):
            raise ConnectionError(  # pragma: no cover
                "Couldn't connect to a collector locally on port {}, are you sure the collector is running?".format(
                    otlp["port"]
                )
            )

        endpoint = "localhost:{}".format(otlp["port"])
        log_provider.add_log_record_processor(
            BatchLogRecordProcessor(
                CustOTLPLogExporterGRPC(endpoint=endpoint, insecure=True).from_level(
                    otlp["from_level"]
                )
            )
        )
        trace_provider.add_span_processor(
            BatchSpanProcessor(CustOTLPSpanExporterGRPC(endpoint=endpoint, insecure=True))
        )
        metric_readers.append(
            PeriodicExportingMetricReader(OTLPMetricExporterGRPC(endpoint=endpoint, insecure=True))
        )

    if console is not None:
        writer = console.get("writer", sys.stdout)

        log_provider.add_log_record_processor(
            BatchLogRecordProcessor(
                CustConsoleLogExporter(
                    out=writer,
                    formatter=lambda record: console_log_formatter(record, console["spans"]),
                ).from_level(console["from_level"])
            )
        )
        if console["spans"]:
            trace_provider.add_span_processor(
                BatchSpanProcessor(
                    CustConsoleSpanExporter(out=writer, formatter=console_span_formatter)
                )
            )
        if console["metrics"]:
            metric_readers.append(
                PeriodicExportingMetricReader(
                    ConsoleMetricExporter(out=writer, formatter=console_metric_formatter)
                )
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
        metric_readers.append(
            PeriodicExportingMetricReader(CustFileMetricExporter(handler=file_handler))
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

    meter_provider = MeterProvider(metric_readers=metric_readers, resource=resource)
    return meter_provider, trace_provider, log_provider, file_handler
