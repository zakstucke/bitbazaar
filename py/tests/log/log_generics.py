import contextlib
import io
import json
import logging
import os
import re
import time
import typing as tp
from abc import ABC, abstractmethod

from bitbazaar.log import GlobalLog
from bitbazaar.log._utils import severity_to_log_level
from bitbazaar.testing import TmpFileManager

if tp.TYPE_CHECKING:  # pragma: no cover
    from _typeshed import StrPath


class GenericLog(tp.TypedDict):
    sid: str | None
    level: str
    body: str


class GenericSpan(tp.TypedDict):
    sid: str
    name: str


class GenericMetric(tp.TypedDict):
    name: str


class Checker(ABC):
    @abstractmethod
    def _logs(self) -> list[GenericLog]:
        """First item is sid, second is log message."""
        pass

    @abstractmethod
    def _spans(self) -> list[GenericSpan]:
        """First item is sid, second is name."""
        pass

    @abstractmethod
    def _metrics(self) -> list[GenericMetric]:
        """First item is sid, second is name."""
        pass

    @abstractmethod
    def read(self) -> tuple[list[GenericLog], list[GenericSpan], list[GenericMetric]]:
        pass

    def sid_is_nully(self, sid: tp.Any):
        if not sid or sid in ["0x0000000000000000", "0"]:
            return True
        return False


class ConsoleChecker(Checker):
    buf: io.StringIO

    def __init__(self, buf: io.StringIO):
        self.buf = buf

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

    def _get_entries(self) -> list[str]:
        self.buf.seek(0)
        contents = self.buf.read()
        return [item for item in contents.split("ENTRY_FIN") if item.strip()]

    def _logs(self) -> list[GenericLog]:
        logs = [item for item in self._get_entries() if "SPAN" not in item and "METRIC" not in item]
        out: list[GenericLog] = []
        for log in logs:
            sid_search = re.search(r"sid=(\w+)", log)
            sid = sid_search.group(1) if sid_search else None
            out.append({"sid": sid, "level": self._coerce_level(log), "body": log})
        return out

    def _spans(self) -> list[GenericSpan]:
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

    def _metrics(self) -> list[GenericMetric]:
        metrics = [item for item in self._get_entries() if "METRIC" in item]
        out: list[GenericMetric] = []
        for metric in metrics:
            name_search = re.search(r"name=(\w+)", metric)
            if not name_search:
                raise ValueError("Couldn't find name for metric! Metric: '{}'".format(metric))
            out.append({"name": name_search.group(1)})
        return out

    def read(self) -> tuple[list[GenericLog], list[GenericSpan], list[GenericMetric]]:
        return self._logs(), self._spans(), self._metrics()


@contextlib.contextmanager
def console_logger(from_level: int, span: bool, metric: bool):
    buf = io.StringIO()
    log = GlobalLog(
        "Python Test",
        "1.0",
        console={"from_level": from_level, "spans": span, "metrics": metric, "writer": buf},
    )
    try:
        yield log, ConsoleChecker(buf)
    finally:
        log.shutdown()


class OLTPChecker(Checker):
    logpath: str
    seek: int

    def __init__(self):
        self.logpath = os.path.join("..", "logs", "otlp.log")
        self.seek = 0

    def _get_entries(self) -> list[tp.Any]:
        entries = []
        with open(self.logpath, "rb") as f:
            # Start from the reset point:
            f.seek(self.seek)

            contents = f.read().decode("utf8")
            for lin in contents.split("\n"):
                lin = lin.strip()
                if lin:
                    try:
                        entries.append(json.loads(lin))
                    except json.JSONDecodeError as e:
                        raise ValueError("Couldn't decode otlp entry: '{}'".format(lin)) from e
        return entries

    def reset_logs(self):
        """The collector is appending, and deleting the contents really confuses the collector (it keeps writing from where it was leaving NULS where we deleted).

        Instead, we record the length of file when reset is called, this becomes the start point for _get_entries() when it's called.
        """
        try:
            # Open the file in binary mode ('rb') to get the file size
            with open(self.logpath, "rb") as file:
                # Move the cursor to the end of the file
                file.seek(0, os.SEEK_END)
                # Get the seek value which represents the end of the file
                self.seek = file.tell()
        except FileNotFoundError:
            pass

    def _logs(self) -> list[GenericLog]:
        logs = []
        for item in self._get_entries():
            if "resourceLogs" in item:
                for resource in item["resourceLogs"]:
                    for scope in resource["scopeLogs"]:
                        for log in scope["logRecords"]:
                            logs.append(log)
        out: list[GenericLog] = []
        for log in logs:
            lvl = logging.getLevelName(severity_to_log_level(log["severityNumber"]))
            if lvl == "WARNING":
                lvl = "WARN"
            out.append(
                {
                    "sid": log["spanId"],
                    "body": log["body"]["stringValue"],
                    "level": lvl,
                }
            )
        return out

    def _spans(self) -> list[GenericSpan]:
        spans = []
        for item in self._get_entries():
            if "resourceSpans" in item:
                for resource in item["resourceSpans"]:
                    for scope in resource["scopeSpans"]:
                        for span in scope["spans"]:
                            spans.append(span)
        out: list[GenericSpan] = []
        for span in spans:
            out.append(
                {
                    "sid": span["spanId"],
                    "name": span["name"],
                }
            )
        return out

    def _metrics(self) -> list[GenericMetric]:
        metrics = []
        for item in self._get_entries():
            if "resourceMetrics" in item:
                for resource in item["resourceMetrics"]:
                    for scope in resource["scopeMetrics"]:
                        for metric in scope["metrics"]:
                            metrics.append({"name": metric["name"]})
        return metrics

    def read(self) -> tuple[list[GenericLog], list[GenericSpan], list[GenericMetric]]:
        # Have to wait for the collector to have actually written the output to file, it does this every second:
        time.sleep(1)
        return self._logs(), self._spans(), self._metrics()


@contextlib.contextmanager
def otlp_logger(from_level: int):
    checker = OLTPChecker()
    checker.reset_logs()

    log = GlobalLog(
        "1.0",
        "Python Test",
        otlp={"from_level": from_level, "port": 4317},
    )

    try:
        yield log, checker
    finally:
        log.shutdown()


class FileChecker(ConsoleChecker):
    logpath: "StrPath"

    def __init__(self, logpath: "StrPath"):
        self.logpath = logpath

    def _get_entries(self):
        with open(self.logpath, "r") as f:
            return [e for e in f.read().split("ENTRY_FIN") if e.strip()]


@contextlib.contextmanager
def file_logger(from_level: int):
    with TmpFileManager() as manager:
        tf = manager.tmpfile(content="", suffix=".log")
        log = GlobalLog(
            "Python Test",
            "1.0",
            file={
                "from_level": from_level,
                "logpath": tf,
                "max_backups": 5,
                "max_bytes": 1000000,
            },
        )
        try:
            yield log, FileChecker(tf)
        finally:
            log.shutdown()
