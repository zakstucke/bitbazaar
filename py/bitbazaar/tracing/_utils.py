from opentelemetry._logs.severity import _STD_TO_OTEL, SeverityNumber, std_to_otel

_OTEL_TO_STD = {v: k for k, v in _STD_TO_OTEL.items()}


def severity_to_log_level(
    sev: SeverityNumber | int,
) -> int:  # pragma: no cover (is covered but not in CI)
    if isinstance(sev, int):
        sev = SeverityNumber(sev)

    return _OTEL_TO_STD[sev]


def log_level_to_severity(level: int) -> SeverityNumber:
    return std_to_otel(level)
