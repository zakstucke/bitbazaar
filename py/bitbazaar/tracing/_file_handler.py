import logging
from logging.handlers import RotatingFileHandler

from opentelemetry.sdk._logs._internal import LogRecord

from ._formatting import file_log_formatter


class CustomRotatingFileHandler(RotatingFileHandler):
    """Custom logging.handlers.RotatingFileHandler that uses open telemetry's log records instead of default logging ones."""

    def emit(self, record: LogRecord) -> None:
        """Subclassed to work with oltp's LogRecord instead of the default logging one.

        https://github.com/python/cpython/blob/3.12/Lib/logging/handlers.py#L65
        """
        try:
            if self.shouldRollover(record):  # type: ignore
                self.doRollover()
            logging.FileHandler.emit(self, record)  # type: ignore
        except Exception:
            self.handleError(record)  # type: ignore

    def format(self, record: LogRecord) -> str:
        """Subclassed to work with oltp's LogRecord and custom formatting func."""
        return file_log_formatter(record).rstrip()
