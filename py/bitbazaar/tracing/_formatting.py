import datetime
import logging
import typing as tp

from opentelemetry.sdk._logs._internal import LogRecord
from rich.console import Console as RichConsole
from rich.markup import escape

from ._utils import severity_to_log_level

rich_console = RichConsole()


def _lvl_to_desc_and_markup(lvl: int) -> tuple[str, str]:
    if lvl <= logging.DEBUG:
        return "DEBUG", "cyan"
    if lvl <= logging.INFO:
        return "INFO", "green"
    if lvl <= logging.WARNING:
        return "WARN", "yellow"
    if lvl <= logging.ERROR:
        return "ERROR", "red"
    else:
        return "CRIT", "bold red"


def console_log_formatter(log: LogRecord) -> str:
    out = ""
    lvl_desc, lvl_markup = (
        _lvl_to_desc_and_markup(severity_to_log_level(log.severity_number))
        if log.severity_number
        else ("UNKNOWN LVL", "white")
    )
    lvl_text = f"{lvl_desc}: "
    out = "[{}]{}[/]".format(
        lvl_markup,
        lvl_text,
    )

    # Make sure it doesn't accidentally match rich markup:
    out += escape(_fmt_body(log, lvl_text)) + "\n"

    out += _fmt_where_parts(log, False)

    with rich_console.capture() as capture:
        rich_console.print(out, end="")
    return capture.get()


def file_log_formatter(log: LogRecord) -> str:
    out = ""

    lvl_desc = (
        _lvl_to_desc_and_markup(severity_to_log_level(log.severity_number))[0]
        if log.severity_number
        else ("UNKNOWN LVL")
    )

    lvl_text = "{}: ".format(lvl_desc)
    out += lvl_text

    out += _fmt_body(log, lvl_text) + "\n"
    out += _fmt_where_parts(log, True)
    return out


def _fmt_body(log: LogRecord, lvl_text: str) -> str:
    lvl_text_space = " " * len(lvl_text)
    body = log.body if log.body else "NO MESSAGE"
    body_lines = body.split("\n")
    body_out = ""
    body_out += body_lines[0] + "\n"
    for line in body_lines[1:]:
        body_out += lvl_text_space + line + "\n"

    # Ignore any extra whitespace at end of body:
    return body_out.rstrip()


def _fmt_where_parts(log: LogRecord, is_file: bool) -> str:
    parts: dict[str, tp.Any] = {}

    # Don't include this extra data when just to console:
    if is_file:
        # Make log.observed_timesamp readable (is ts_since the epoch):
        parts["ts"] = datetime.datetime.fromtimestamp(log.observed_timestamp / 1e9).isoformat()

        if log.trace_id is not None:
            parts["tid"] = log.trace_id

        if log.span_id is not None:
            parts["sid"] = log.span_id

    # Always include extra attributes if they've been supplied:
    if log.attributes:
        for key, value in log.attributes.items():
            parts[key] = value

    if parts:
        parts_str = " ".join(f"{k}={v}" for k, v in parts.items())
        # Only include color for console:
        if is_file:
            return f"    where {parts_str}\n"
        else:
            return f"[italic]    where {parts_str}[/]\n"
    else:
        return ""
