import logging

from opentelemetry.sdk.trace import TracerProvider

from bitbazaar import utils

from ._setup import Args, prepare_provider


class GlobalLog:
    provider: TracerProvider

    def __init__(self, args: Args):
        """Initialize tracing for a project.

        NOTE: only the otlp sink logs spans, but file logger will include sid in logs, to allow finding them from oltp (e.g. debug in file, but only info up in oltp.)
        """
        self.provider = prepare_provider(args)

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
