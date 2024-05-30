import { describe, expect, it } from "bun:test";

import { GlobalLog, LOG, type LogLevel } from "bitbazaar/log";

describe("Logging/Tracing", () => {
    it.each([
        ["DEBUG", ["DEBUG", "INFO", "WARN", "ERROR"]],
        ["INFO", ["INFO", "WARN", "ERROR"]],
        ["WARN", ["WARN", "ERROR"]],
        ["ERROR", ["ERROR"]],
        [null, []],
    ])(
        "Logs: min level %i should enable logs for: %i",
        async (level_from_or_null: string | null, expected_enabled: string[]) => {
            const logs: string[] = [];
            new GlobalLog({
                console: level_from_or_null
                    ? {
                          level_from: level_from_or_null as LogLevel,
                          custom_out: (message) => {
                              logs.push(message);
                          },
                      }
                    : undefined,
                // Not using in this test, just can't disable:
                otlp: {
                    endpoint: "http://localhost:4318",
                    // otlp can't be null, debug when null
                    level_from: "INFO",
                    service_name: "js-test",
                    service_version: "1.0.0",
                },
            });

            LOG.debug("DEBUG");
            LOG.info("INFO");
            LOG.warn("WARN");
            LOG.error("ERROR");
            expect(logs).toEqual(expected_enabled);

            // Empty:
            logs.length = 0;

            // All console logs should have been overridden to also use the global logger:
            console.debug("DEBUG");
            console.info("INFO");
            console.warn("WARN");
            console.error("ERROR");
            expect(logs).toEqual(expected_enabled);
        },
    );
    it("Logs: multiple non string args should work", async () => {
        const logs: string[] = [];
        new GlobalLog({
            console: {
                level_from: "DEBUG",
                custom_out: (message) => {
                    logs.push(message);
                },
            },
            otlp: {
                endpoint: "http://localhost:4318",
                level_from: "INFO",
                service_name: "js-test",
                service_version: "1.0.0",
            },
        });

        LOG.debug("DEBUG", 1, 2, 3);
        LOG.info("INFO", 1, 2, 3);
        LOG.warn("WARN", 1, 2, 3);
        LOG.error("ERROR", 1, 2, 3);
        expect(logs).toEqual(["DEBUG 1 2 3", "INFO 1 2 3", "WARN 1 2 3", "ERROR 1 2 3"]);
    });
    it("Logs: oltp", async () => {
        // Just confirm nothing errors, when trying to flush and measure output from tests etc seems to cause problems with bun test.
        new GlobalLog({
            otlp: {
                endpoint: "http://localhost:4318",
                level_from: "WARN",
                service_name: "js-test",
                service_version: "1.0.0",
            },
        });
        const meter = LOG.meter("test");
        const counter = meter.createCounter("test_counter");
        counter.add(1);
        LOG.withSpan("test", (span) => {
            LOG.debug("DEBUG");
            LOG.warn("WARN");
        });
        await LOG.withSpanAsync("test", async (span) => {
            LOG.info("INFO");
            LOG.error("ERROR");
        });
    });
});
