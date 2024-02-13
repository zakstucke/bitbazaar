import typing as tp

from opentelemetry.exporter.otlp.proto.grpc._log_exporter import (
    OTLPLogExporter as OTLPLogExporterGRPC,
)
from opentelemetry.exporter.otlp.proto.grpc.trace_exporter import (
    OTLPSpanExporter as OTLPSpanExporterGRPC,
)
from opentelemetry.sdk._logs import LogData
from opentelemetry.sdk._logs.export import (
    ConsoleLogExporter,
    LogExporter,
    LogExportResult,
)
from opentelemetry.sdk.metrics._internal.export import MetricExportResult
from opentelemetry.sdk.metrics._internal.point import MetricsData
from opentelemetry.sdk.metrics.export import MetricExporter
from opentelemetry.sdk.trace import ReadableSpan
from opentelemetry.sdk.trace.export import (
    ConsoleSpanExporter,
    SpanExporter,
    SpanExportResult,
)

from bitbazaar import misc

from ._file_handler import CustomRotatingFileHandler
from ._utils import log_level_to_severity


class CustOTLPLogExporterGRPC(OTLPLogExporterGRPC):  # pragma: no cover (is covered but not in CI)
    _filter_from_level: int | None

    @misc.copy_sig(OTLPLogExporterGRPC.__init__)
    def __init__(self, *args, **kwargs):  # type: ignore
        self._filter_from_level = None
        super().__init__(*args, **kwargs)

    def from_level(self, level: int) -> tp.Self:
        self._filter_from_level = level
        return self

    def export(self, log_data: tp.Sequence[LogData]) -> LogExportResult:
        filtered = fil_log_data(log_data, self._filter_from_level)
        return super().export(filtered)


class CustOTLPSpanExporterGRPC(OTLPSpanExporterGRPC):  # pragma: no cover (is covered but not in CI)
    pass


class CustConsoleLogExporter(ConsoleLogExporter):
    _filter_from_level: int | None

    @misc.copy_sig(ConsoleLogExporter.__init__)
    def __init__(self, *args, **kwargs):  # type: ignore
        self._filter_from_level = None
        super().__init__(*args, **kwargs)

    def from_level(self, level: int) -> tp.Self:
        self._filter_from_level = level
        return self

    def export(self, log_data: tp.Sequence[LogData]) -> LogExportResult:
        filtered = fil_log_data(log_data, self._filter_from_level)
        return super().export(filtered)


class CustConsoleSpanExporter(ConsoleSpanExporter):
    pass


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


class CustFileMetricExporter(MetricExporter):
    _file_handler: CustomRotatingFileHandler

    def __init__(self, handler: CustomRotatingFileHandler):
        self._file_handler = handler
        super().__init__()

    def export(
        self, metrics_data: MetricsData, timeout_millis: float = 10000, **kwargs: tp.Any
    ) -> MetricExportResult:
        self._file_handler.emit(metrics_data)
        return MetricExportResult.SUCCESS

    def force_flush(self, timeout_millis: float = 10000) -> bool:
        self._file_handler.flush()
        return True

    def shutdown(self, timeout_millis: float = 30000, **kwargs: tp.Any) -> None:
        self._file_handler.close()


def fil_log_data(data: tp.Sequence[LogData], lvl: int | None) -> tp.Sequence[LogData]:
    assert lvl is not None, "from_level() should have been called!"

    out = []
    lvl_to_sev_value = log_level_to_severity(lvl).value
    for log in data:
        sev = log.log_record.severity_number
        if sev is None or sev.value >= lvl_to_sev_value:
            out.append(log)

    return out
