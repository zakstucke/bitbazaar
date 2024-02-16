"""Global tracing implementation for open telemetry, console and file sinks."""

from ._global_log import LOG, GlobalLog

__all__ = ["GlobalLog", "LOG"]
