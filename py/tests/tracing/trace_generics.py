import contextlib
import logging
import re
import tempfile
import typing as tp
from abc import ABC, abstractmethod

from bitbazaar._testing import BUF
from bitbazaar.tracing import GlobalLog
from bitbazaar.tracing._utils import severity_to_log_level
from opentelemetry.sdk._logs import LogData
from opentelemetry.sdk.trace import ReadableSpan


class GenericLog(tp.TypedDict):
    sid: str | None
    level: str
    body: str


class GenericSpan(tp.TypedDict):
    sid: str
    name: str


class Checker(ABC):
    @abstractmethod
    def logs(self) -> list[GenericLog]:
        """First item is sid, second is log message."""
        pass

    @abstractmethod
    def spans(self) -> list[GenericSpan]:
        """First item is sid, second is name."""
        pass


class ConsoleChecker(Checker):
    def __init__(self):
        pass

    def _get_entries(self):
        return BUF

    def _coerce_level(self, log: str) -> str:
        if "DEBUG" in log:
            return "DEBUG"
        if "INFO" in log:
            return "INFO"
        if "WARN" in log:
            return "WARN"
        if "ERROR" in log:
            return "ERROR"
        if "CRITICAL" in log:
            return "CRITICAL"

        raise ValueError(f"Couldn't find level in log: '{log}'")

    def logs(self) -> list[GenericLog]:
        logs = [item for item in self._get_entries() if "SPAN" not in item]
        out: list[GenericLog] = []
        for log in logs:
            sid_search = re.search(r"sid=(\w+)", log)
            sid = sid_search.group(1) if sid_search else None
            out.append({"sid": sid, "level": self._coerce_level(log), "body": log})
        return out

    def spans(self) -> list[GenericSpan]:
        spans = [item for item in self._get_entries() if "SPAN" in item]
        out: list[GenericSpan] = []
        for span in spans:
            sid_search = re.search(r"sid=(\w+)", span)
            # E.g. SPAN: ($name)
            name_search = re.search(r"SPAN: \((.*?)\)", span)
            if not name_search:
                raise ValueError("Couldn't find name for span! Span: '{}'".format(span))
            if sid_search:
                out.append(
                    {
                        "sid": sid_search.group(1),
                        "name": name_search.group(1),
                    }
                )
            else:
                raise ValueError("Couldn't find sid for span! Span: '{}'".format(span))
        return out


@contextlib.contextmanager
def console_logger(from_level: int, span: bool):
    BUF.clear()
    log = GlobalLog("MA SERVICE", console={"from_level": from_level, "spans": span})
    try:
        yield log, ConsoleChecker()
    finally:
        log.shutdown()


class OLTPChecker(Checker):
    def __init__(self):
        pass

    def _get_entries(self):
        return BUF

    def logs(self) -> list[GenericLog]:
        logs = [item for item in self._get_entries() if isinstance(item, LogData)]
        out: list[GenericLog] = []
        for log in logs:
            sid = str(log.log_record.span_id) if log.log_record.span_id is not None else None
            if log.log_record.severity_number is None:
                raise ValueError(f"Log record doesn't have severity number! Log: '{log}'")
            lvl = logging.getLevelName(severity_to_log_level(log.log_record.severity_number))
            if lvl == "WARNING":
                lvl = "WARN"
            out.append(
                {
                    "sid": sid,
                    "body": str(log.log_record.body),
                    "level": lvl,
                }
            )
        return out

    def spans(self) -> list[GenericSpan]:
        spans = [item for item in self._get_entries() if isinstance(item, ReadableSpan)]
        out: list[GenericSpan] = []
        for span in spans:
            if span.context is not None:
                out.append(
                    {
                        "sid": str(span.context.span_id),
                        "name": span.name,
                    }
                )
            else:
                raise ValueError("Couldn't find sid for span! Span: '{}'".format(span))
        return out


@contextlib.contextmanager
def otlp_logger(from_level: int):
    BUF.clear()
    log = GlobalLog("MA SERVICE", otlp={"from_level": from_level, "headers": {}, "url": "foo"})
    try:
        yield log, OLTPChecker()
    finally:
        log.shutdown()


class FileChecker(ConsoleChecker):
    logpath: str

    def __init__(self, logpath: str):
        self.logpath = logpath

    def _get_entries(self):
        with open(self.logpath, "r") as f:
            return [e for e in f.read().split("ENTRY_FIN") if e.strip()]


@contextlib.contextmanager
def file_logger(from_level: int):
    temp = tempfile.NamedTemporaryFile()
    log = GlobalLog(
        "MA SERVICE",
        file={
            "from_level": from_level,
            "logpath": temp.name,
            "max_backups": 5,
            "max_bytes": 1000000,
        },
    )
    try:
        yield log, FileChecker(temp.name)
    finally:
        log.shutdown()
