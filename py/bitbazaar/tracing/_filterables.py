import pathlib
import typing as tp

from opentelemetry.exporter.otlp.proto.grpc._log_exporter import OTLPLogExporter
from opentelemetry.sdk._logs import LogData

# TODO use InMemoryLogExporter instead for console
from opentelemetry.sdk._logs.export import (
    ConsoleLogExporter,
    LogExporter,
    LogExportResult,
)

from bitbazaar import utils

from ._file_handler import CustomRotatingFileHandler
from ._utils import log_level_to_severity


class FilterableOTLPLogExporter(OTLPLogExporter):
    _filter_from_level: int | None

    @utils.copy_sig(OTLPLogExporter.__init__)
    def __init__(self, *args, **kwargs):  # type: ignore
        self._filter_from_level = None
        super().__init__(*args, **kwargs)

    def from_level(self, level: int) -> "FilterableOTLPLogExporter":
        self._filter_from_level = level
        return self

    def export(self, log_data: tp.Sequence[LogData]) -> LogExportResult:
        return super().export(fil_log_data(log_data, self._filter_from_level))


class FilterableConsoleLogExporter(ConsoleLogExporter):
    _filter_from_level: int | None

    @utils.copy_sig(ConsoleLogExporter.__init__)
    def __init__(self, *args, **kwargs):  # type: ignore
        self._filter_from_level = None
        super().__init__(*args, **kwargs)

    def from_level(self, level: int) -> "FilterableConsoleLogExporter":
        self._filter_from_level = level
        return self

    def export(self, log_data: tp.Sequence[LogData]) -> LogExportResult:
        return super().export(fil_log_data(log_data, self._filter_from_level))


class FilterableFileLogExporter(LogExporter):
    _filter_from_level: int | None
    _file_handler: CustomRotatingFileHandler

    def __init__(self, filepath: pathlib.Path, max_bytes: int, max_backups: int):
        self._filter_from_level = None
        self._file_handler = CustomRotatingFileHandler(
            filepath, maxBytes=max_bytes, backupCount=max_backups
        )
        super().__init__()

    def from_level(self, level: int) -> "FilterableFileLogExporter":
        self._filter_from_level = level
        return self

    def export(self, log_data: tp.Sequence[LogData]) -> LogExportResult:
        for log in log_data:
            self._file_handler.emit(log.log_record)
        return LogExportResult.SUCCESS

    def shutdown(self):
        self._file_handler.close()
        super().shutdown()


def fil_log_data(data: tp.Sequence[LogData], lvl: int | None) -> tp.Sequence[LogData]:
    if lvl is None:
        return data

    out = []
    lvl_to_sev_value = log_level_to_severity(lvl).value
    for log in data:
        sev = log.log_record.severity_number
        if sev is None or sev.value >= lvl_to_sev_value:
            out.append(log)

    return out
