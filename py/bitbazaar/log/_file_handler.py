import logging
from logging.handlers import RotatingFileHandler

from opentelemetry.sdk._logs._internal import LogRecord
from opentelemetry.sdk.metrics._internal.point import MetricsData
from opentelemetry.sdk.trace import ReadableSpan

from ._formatting import file_log_formatter, file_metric_formatter, file_span_formatter


class CustomRotatingFileHandler(RotatingFileHandler):
    """Custom logging.handlers.RotatingFileHandler that uses open telemetry's log records (or spans) instead of default logging ones."""

    def emit(self, record: LogRecord | ReadableSpan | MetricsData) -> None:
        """Subclassed to work with oltp's LogRecord or ReadableSpan instead of the default logging one.

        https://github.com/python/cpython/blob/3.12/Lib/logging/handlers.py#L65
        """
        if self.shouldRollover(record):  # type: ignore
            self.doRollover()  # pragma: no cover
        logging.FileHandler.emit(self, record)  # type: ignore

    def format(self, record: LogRecord | ReadableSpan | MetricsData) -> str:
        """Subclassed to work with oltp's LogRecord or ReadableSpan and custom formatting func."""
        if isinstance(record, LogRecord):
            return file_log_formatter(record).rstrip()
        elif isinstance(record, ReadableSpan):
            return file_span_formatter(record).rstrip()
        else:
            return file_metric_formatter(record).rstrip().replace("\n", "")
