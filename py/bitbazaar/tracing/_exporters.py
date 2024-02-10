import typing as tp

from opentelemetry.exporter.otlp.proto.grpc._log_exporter import OTLPLogExporter
from opentelemetry.exporter.otlp.proto.grpc.trace_exporter import OTLPSpanExporter
from opentelemetry.sdk._logs import LogData
from opentelemetry.sdk._logs.export import (
    ConsoleLogExporter,
    LogExporter,
    LogExportResult,
)
from opentelemetry.sdk.trace import ReadableSpan
from opentelemetry.sdk.trace.export import (
    ConsoleSpanExporter,
    SpanExporter,
    SpanExportResult,
)

import bitbazaar._testing
from bitbazaar import utils

from ._file_handler import CustomRotatingFileHandler
from ._formatting import console_log_formatter, console_span_formatter
from ._utils import log_level_to_severity


class CustOTLPLogExporter(OTLPLogExporter):
    _filter_from_level: int | None

    @utils.copy_sig(OTLPLogExporter.__init__)
    def __init__(self, *args, **kwargs):  # type: ignore
        self._filter_from_level = None
        super().__init__(*args, **kwargs)

    def from_level(self, level: int) -> tp.Self:
        self._filter_from_level = level
        return self

    def export(self, log_data: tp.Sequence[LogData]) -> LogExportResult:
        filtered = fil_log_data(log_data, self._filter_from_level)
        if bitbazaar._testing.IS_TEST:
            bitbazaar._testing.BUF.extend(filtered)
            return LogExportResult.SUCCESS
        return super().export(filtered)  # pragma: no cover


class CustOTLPSpanExporter(OTLPSpanExporter):
    def export(self, spans: tp.Sequence[ReadableSpan]) -> SpanExportResult:
        if bitbazaar._testing.IS_TEST:
            bitbazaar._testing.BUF.extend(spans)
            return SpanExportResult.SUCCESS
        return super().export(spans)  # pragma: no cover


class CustConsoleLogExporter(ConsoleLogExporter):
    _filter_from_level: int | None

    @utils.copy_sig(ConsoleLogExporter.__init__)
    def __init__(self, *args, **kwargs):  # type: ignore
        self._filter_from_level = None
        super().__init__(*args, **kwargs)

    def from_level(self, level: int) -> tp.Self:
        self._filter_from_level = level
        return self

    def export(self, log_data: tp.Sequence[LogData]) -> LogExportResult:
        filtered = fil_log_data(log_data, self._filter_from_level)
        if bitbazaar._testing.IS_TEST:
            bitbazaar._testing.BUF.extend(
                [console_log_formatter(log.log_record) for log in filtered]
            )
            return LogExportResult.SUCCESS
        return super().export(filtered)  # pragma: no cover


class CustConsoleSpanExporter(ConsoleSpanExporter):
    def export(self, spans: tp.Sequence[ReadableSpan]) -> SpanExportResult:
        if bitbazaar._testing.IS_TEST:
            bitbazaar._testing.BUF.extend([console_span_formatter(span) for span in spans])
            return SpanExportResult.SUCCESS
        return super().export(spans)  # pragma: no cover


class CustFileLogExporter(LogExporter):
    _filter_from_level: int | None
    _file_handler: CustomRotatingFileHandler

    def __init__(self, handler: CustomRotatingFileHandler):
        self._filter_from_level = None
        self._file_handler = handler
        super().__init__()

    def from_level(self, level: int) -> tp.Self:
        self._filter_from_level = level
        return self

    def export(self, log_data: tp.Sequence[LogData]) -> LogExportResult:
        filtered = fil_log_data(log_data, self._filter_from_level)
        for log in filtered:
            self._file_handler.emit(log.log_record)
        return LogExportResult.SUCCESS

    def shutdown(self) -> None:
        self._file_handler.close()
        super().shutdown()


class CustFileSpanExporter(SpanExporter):
    _file_handler: CustomRotatingFileHandler

    def __init__(self, handler: CustomRotatingFileHandler):
        self._file_handler = handler
        super().__init__()

    def export(self, spans: tp.Sequence[ReadableSpan]) -> SpanExportResult:
        for span in spans:
            self._file_handler.emit(span)
        return SpanExportResult.SUCCESS

    def shutdown(self) -> None:
        self._file_handler.close()
        return super().shutdown()


def fil_log_data(data: tp.Sequence[LogData], lvl: int | None) -> tp.Sequence[LogData]:
    assert lvl is not None, "from_level() should have been called!"

    out = []
    lvl_to_sev_value = log_level_to_severity(lvl).value
    for log in data:
        sev = log.log_record.severity_number
        if sev is None or sev.value >= lvl_to_sev_value:
            out.append(log)

    return out
