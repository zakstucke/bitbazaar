import logging
import typing as tp

import pytest
from bitbazaar.log import GlobalLog
from bitbazaar.misc import in_ci

from .log_generics import Checker, console_logger, file_logger, otlp_logger

ttl_cases: list[tuple[str, tp.Callable[[int], tp.ContextManager[tuple[GlobalLog, Checker]]]]] = [
    ("console", lambda log_level: console_logger(log_level, span=False, metric=False)),
    ("console", lambda log_level: console_logger(log_level, span=True, metric=False)),
    ("file", lambda log_level: file_logger(log_level)),
]

# OTLP only when have access to the local collector:
if not in_ci():
    ttl_cases.append(
        (
            "otlp",
            lambda log_level: otlp_logger(log_level),
        )
    )


def test_log_global_accessor():
    from bitbazaar.log import LOG

    GlobalLog("foo", "1.0.0")

    # Make sure doesn't error and can clearly find it:
    LOG.debug("Hello, world!")


@pytest.mark.parametrize("desc, log_manager", ttl_cases)
def test_log_level(
    desc: str,
    log_manager: tp.Callable[[int], tp.ContextManager[tuple[GlobalLog, Checker]]],
):
    """Confirm levels are filtered correctly."""
    all_levels = [
        ["DEBUG", "IS_D"],
        ["INFO", "IS_I"],
        ["WARN", "IS_W"],
        ["ERROR", "IS_E"],
        ["CRITICAL", "IS_C"],
    ]

    for from_level, should_match in [
        (logging.NOTSET, all_levels),
        (logging.DEBUG, all_levels),
        (logging.INFO, all_levels[1:]),
        (logging.WARN, all_levels[2:]),
        (logging.ERROR, all_levels[3:]),
        (logging.CRITICAL, all_levels[4:]),
    ]:
        with log_manager(from_level) as (log, checker):
            log.debug("IS_D")
            log.info("IS_I")
            log.warn("IS_W")
            log.error("IS_E")
            log.crit("IS_C")
            log.flush()

            logs, _, _ = checker.read()
            assert len(logs) == len(should_match)
            for i, (lvl, msg) in enumerate(should_match):
                assert lvl == logs[i]["level"] and msg in logs[i]["body"]


tts_cases: list[tuple[str, tp.Callable[[int], tp.ContextManager[tuple[GlobalLog, Checker]]]]] = [
    ("console", lambda log_level: console_logger(log_level, span=True, metric=False)),
    ("file", lambda log_level: file_logger(log_level)),
]

# OTLP only when have access to the local collector:
if not in_ci():
    tts_cases.append(
        ("otlp", lambda log_level: otlp_logger(log_level)),
    )


@pytest.mark.parametrize("desc, log_manager", tts_cases)
def test_log_spans(
    desc: str,
    log_manager: tp.Callable[[int], tp.ContextManager[tuple[GlobalLog, Checker]]],
):
    """Confirm spans are recorded and mapped properly."""
    with log_manager(logging.DEBUG) as (log, checker):
        log.debug("BEFORE")
        with log.span("MYSPAN", attributes={"MY_SPAN_EXTRA": 1}):
            log.debug("INSIDE", extra={"MY_LOG_EXTRA": 2})
            with log.span("NESTED_SPAN"):
                log.debug("NESTED_LOG\n\nMULTILINE!")
        log.debug("AFTER")
        try:
            with log.span("SPAN_WILL_RAISE"):
                raise ValueError("This is an error")
        except ValueError:
            pass
        log.flush()

        logs, spans, _ = checker.read()
        assert len(logs) == 4
        assert len(spans) == 3

        # First log should be before any spans and not attached:
        assert checker.sid_is_nully(logs[0]["sid"])

        # Nested should come through first:
        assert spans[0]["name"] == "NESTED_SPAN"
        assert spans[0]["sid"] == logs[2]["sid"]

        # Then outer:
        assert spans[1]["name"] == "MYSPAN"
        assert spans[1]["sid"] == logs[1]["sid"]

        # Error span should still come through:
        assert spans[2]["name"] == "SPAN_WILL_RAISE"

        # Last log should be after all spans and not attached:
        assert checker.sid_is_nully(logs[3]["sid"])


ttm_cases: list[tuple[str, tp.Callable[[int], tp.ContextManager[tuple[GlobalLog, Checker]]]]] = [
    ("console", lambda log_level: console_logger(log_level, span=True, metric=True)),
    ("file", lambda log_level: file_logger(log_level)),
]

# OTLP only when have access to the local collector:
if not in_ci():
    ttm_cases.append(
        ("otlp", lambda log_level: otlp_logger(log_level)),
    )


@pytest.mark.parametrize("desc, log_manager", ttm_cases)
def test_log_metrics(
    desc: str,
    log_manager: tp.Callable[[int], tp.ContextManager[tuple[GlobalLog, Checker]]],
):
    """Confirm metrics are recorded and mapped properly."""
    with log_manager(logging.DEBUG) as (log, checker):
        meter = log.get_meter("my meter")

        counter = meter.create_counter("my_counter")
        counter.add(1)

        histogram = meter.create_histogram("my_histogram")
        histogram.record(99.9)

        log.flush()

        _, _, metrics = checker.read()
        assert len(metrics) == 2
        assert metrics[0]["name"] == "my_counter"
        assert metrics[1]["name"] == "my_histogram"
