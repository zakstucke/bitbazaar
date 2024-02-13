import json
import logging
import typing as tp

from opentelemetry import trace as trace_api
from opentelemetry.sdk import util as open_util
from opentelemetry.sdk._logs._internal import LogRecord
from opentelemetry.sdk.metrics._internal.point import MetricsData
from opentelemetry.sdk.trace import ReadableSpan, StatusCode
from rich.console import Console as RichConsole
from rich.markup import escape

import bitbazaar._testing

from ._utils import severity_to_log_level

CONSOLE: RichConsole | None = None


def get_console() -> RichConsole:
    global CONSOLE
    if CONSOLE is None:
        # No color if testing to allow regexes to work:
        CONSOLE = RichConsole(color_system="auto" if not bitbazaar._testing.IS_TEST else None)
    return CONSOLE


def file_metric_formatter(metrics: MetricsData) -> str:
    outs: list[str] = []
    for resource_metrics in metrics.resource_metrics:
        for scope_metrics in resource_metrics.scope_metrics:
            for metric in scope_metrics.metrics:
                parts = {}
                parts["name"] = metric.name
                if metric.description:  # pragma: no cover
                    parts["description"] = metric.description
                if metric.unit:  # pragma: no cover
                    parts["unit"] = metric.unit
                parts["data"] = json.loads(metric.data.to_json())
                out = "METRIC: {}".format(" ".join([f"{k}={v}" for k, v in parts.items()]))

                # Useful for splitting during tests:
                if bitbazaar._testing.IS_TEST:
                    out += "ENTRY_FIN"
                outs.append(out)
    return "\n".join(outs)


def console_metric_formatter(metrics: MetricsData) -> str:
    outs: list[str] = []
    for resource_metrics in metrics.resource_metrics:
        for scope_metrics in resource_metrics.scope_metrics:
            for metric in scope_metrics.metrics:
                parts = {}
                parts["name"] = metric.name
                if metric.description:  # pragma: no cover
                    parts["description"] = metric.description
                if metric.unit:  # pragma: no cover
                    parts["unit"] = metric.unit
                parts["data"] = json.loads(metric.data.to_json())
                out = "METRIC: {}".format(" ".join([f"{k}={v}" for k, v in parts.items()]))

                # Useful for splitting during tests:
                if bitbazaar._testing.IS_TEST:
                    out += "ENTRY_FIN"

                outs.append(out)

    out = "\n".join(outs)
    console = get_console()
    with console.capture() as capture:
        console.print("[dim]" + out + "[/]")
    out = capture.get()

    return out


def console_span_formatter(span: ReadableSpan) -> str:
    parts = _span_parts(span, False)
    out = f"[bold]SPAN: [/]({span.name}) "
    out += " ".join(f"{k}={v}" for k, v in parts.items() if v is not None)

    console = get_console()
    with console.capture() as capture:
        console.print("[dim]" + out + "[/]")
    out = capture.get()

    # Useful for splitting during tests:
    if bitbazaar._testing.IS_TEST:
        out += "ENTRY_FIN"

    return out


def file_span_formatter(span: ReadableSpan) -> str:
    parts = _span_parts(span, True)
    out = f"SPAN: ({span.name}) "
    out += " ".join(f"{k}={v}" for k, v in parts.items() if v is not None)
    out += "\n"

    # Useful for splitting during tests:
    if bitbazaar._testing.IS_TEST:
        out += "ENTRY_FIN"

    return out


def console_log_formatter(log: LogRecord, show_sids: bool) -> str:
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

    out += _fmt_where_parts(log, False, show_sids)

    console = get_console()
    with console.capture() as capture:
        console.print(out, end="")

    out = capture.get()

    # Useful for splitting during tests:
    if bitbazaar._testing.IS_TEST:
        out += "ENTRY_FIN"

    return out


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
    out += _fmt_where_parts(log, True, True)

    # Useful for splitting during tests:
    if bitbazaar._testing.IS_TEST:
        out += "ENTRY_FIN"

    return out


def _span_parts(span: ReadableSpan, is_file: bool) -> dict[str, tp.Any]:
    parts = {}
    if span._context is not None:
        parts["sid"] = f"0x{trace_api.format_span_id(span._context.span_id)}"
        # Don't bother including trace info if console:
        if is_file:
            parts["tid"] = f"0x{trace_api.format_trace_id(span._context.trace_id)}"
            trace_state = repr(span._context.trace_state)
            if trace_state:
                parts["trace_state"] = trace_state

    if span.parent is not None:
        parts["pid"] = f"0x{trace_api.format_span_id(span.parent.span_id)}"

    # Start only needed in file (console elapsed is enough):
    if is_file and span._start_time:
        parts["start"] = open_util.ns_to_iso_str(span._start_time)

    if span._start_time and span._end_time:
        elapsed_ns = span._end_time - span._start_time
        parts["elapsed"] = _format_duration(elapsed_ns)

    if span._status.status_code is not StatusCode.UNSET:
        if span._status.description:
            parts["status"] = f"{span._status.status_code.name}: {span._status.description}"
        else:  # pragma: no cover
            parts["status"] = span._status.status_code.name

    # Kind seems pretty useless, just in file:
    if is_file:
        parts["kind"] = str(span.kind)

    attrs = span._format_attributes(span._attributes)
    if attrs:
        parts["attrs"] = attrs

    events = span._format_events(span._events)
    if events:
        parts["events"] = events

    links = span._format_links(span._links)
    if links:  # pragma: no cover
        parts["links"] = links

    return parts


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
        return "CRITICAL", "bold red"


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


def _fmt_where_parts(log: LogRecord, is_file: bool, show_sids: bool) -> str:
    parts: dict[str, tp.Any] = {}

    if show_sids and log.span_id is not None:
        parts["sid"] = f"0x{trace_api.format_span_id(log.span_id)}"

    # Don't include this extra data when just to console:
    if is_file:
        # Make log.observed_timesamp readable (is ts_since the epoch):
        parts["ts"] = open_util.ns_to_iso_str(log.observed_timestamp)

        if log.trace_id is not None:
            parts["tid"] = f"0x{trace_api.format_trace_id(log.trace_id)}"

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
            return f"[dim italic]    where {parts_str}[/]\n"
    else:
        return ""


def _format_duration(nanoseconds: int) -> str:  # pragma: no cover
    if nanoseconds < 1000:
        return f"{nanoseconds}ns"
    elif nanoseconds < 1000000:
        return f"{nanoseconds / 1000}μs"
    elif nanoseconds < 1000000000:
        return f"{nanoseconds / 1000000:.1f}ms"
    else:
        return f"{nanoseconds / 1000000000:.2f}s"
