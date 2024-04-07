import { ZoneContextManager } from "@opentelemetry/context-zone";
import { type BufferConfig, type Tracer, WebTracerProvider } from "@opentelemetry/sdk-trace-web";

import {
    CompositePropagator,
    W3CBaggagePropagator,
    W3CTraceContextPropagator,
} from "@opentelemetry/core";
import { DocumentLoadInstrumentation } from "@opentelemetry/instrumentation-document-load";
import { FetchInstrumentation } from "@opentelemetry/instrumentation-fetch";
import { UserInteractionInstrumentation } from "@opentelemetry/instrumentation-user-interaction";

import type { Span } from "@opentelemetry/api";
import { Resource } from "@opentelemetry/resources";
import { SemanticResourceAttributes } from "@opentelemetry/semantic-conventions";

import { registerInstrumentations } from "@opentelemetry/instrumentation";

import { BatchSpanProcessor } from "@opentelemetry/sdk-trace-base";

import type { LogAttributes, Logger } from "@opentelemetry/api-logs";
import { BatchLogRecordProcessor, LoggerProvider } from "@opentelemetry/sdk-logs";

import type { Meter, MeterOptions } from "@opentelemetry/api";
import { OTLPLogExporter } from "@opentelemetry/exporter-logs-otlp-http";
import { OTLPMetricExporter } from "@opentelemetry/exporter-metrics-otlp-http";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-http";
import type { OTLPExporterNodeConfigBase } from "@opentelemetry/otlp-exporter-base";
import { MeterProvider, PeriodicExportingMetricReader } from "@opentelemetry/sdk-metrics";

// Create a new proxy with a handler
let _LOG: GlobalLog | undefined;

export type LogLevel = "DEBUG" | "INFO" | "WARN" | "ERROR";

interface ConsoleArgs {
    level_from: LogLevel;
    custom_out?: (message: string, ...optionalParams: any[]) => void;
}

interface OltpArgs {
    level_from: LogLevel;
    endpoint: string; // E.g. "/otlp or http://localhost:4317/otlp"
    service_name: string; // E.g. "web"
    service_version: string; // E.g. "1.0.0"
}

type ConsoleFns = Record<
    "log" | "debug" | "info" | "warn" | "error",
    (message: string, ...optionalParams: any[]) => void
>;

class GlobalLog {
    loggerProvider: LoggerProvider;
    logger: Logger;
    tracerProvider: WebTracerProvider;
    tracer: Tracer;
    meterProvider: MeterProvider;

    console: ConsoleArgs | null;
    oltp: OltpArgs;

    /// We override global console functions to do filtering and emit to oltp, need to keep access to the inner ones:
    orig_console_fns: ConsoleFns;

    /**
     * Get a new Meter instance to record metrics with.
     *
     * @example
     *  ```typescript
     * const meter = globalLog.getMeter("example-meter");
     * const counter = meter.createCounter('metric_name');
     * counter.add(10, { 'key': 'value' });
     * ```
     */
    meter(
        name: string,
        opts:
            | MeterOptions
            | {
                  version?: string; // The version of the meter.
              } = {},
    ): Meter {
        return this.meterProvider.getMeter(
            name,
            typeof opts === "object" && "version" in opts ? opts.version : undefined,
            opts as MeterOptions,
        );
    }

    /**
     * Run a sync callback inside a span.
     */
    withSpan<T>(name: string, cb: (span) => T): T {
        return this.tracer.startActiveSpan(name, (span: Span) => {
            const result = cb(span);
            span.end();
            return result;
        });
    }

    /**
     * Run an async callback inside a span.
     */
    withSpanAsync<T>(name: string, cb: (span) => Promise<T>): Promise<T> {
        return this.tracer.startActiveSpan(name, (span: Span) => {
            return cb(span).then((result) => {
                span.end();
                return result;
            });
        });
    }

    /** Log a debug message. */
    debug(message: string, attributes?: LogAttributes) {
        this._log_inner("DEBUG", message, attributes);
    }

    /** Log an info message. */
    info(message: string, attributes?: LogAttributes) {
        this._log_inner("INFO", message, attributes);
    }

    /** Log a warning message. */
    warn(message: string, attributes?: LogAttributes) {
        this._log_inner("WARN", message, attributes);
    }

    /** Log an error message. */
    error(message: string, attributes?: LogAttributes) {
        this._log_inner("ERROR", message, attributes);
    }

    _log_inner(severityText: LogLevel, message: string, attributes: LogAttributes | undefined) {
        // Log to console if enabled:
        if (this.console) {
            let emit = false;
            let emitter: Exclude<ConsoleArgs["custom_out"], undefined>;
            switch (this.console.level_from) {
                case "DEBUG": {
                    emit = true;
                    emitter = this.orig_console_fns.debug;
                    break;
                }
                case "INFO": {
                    emit = severityText !== "DEBUG";
                    emitter = this.orig_console_fns.info;
                    break;
                }
                case "WARN": {
                    emit = severityText === "WARN" || severityText === "ERROR";
                    emitter = this.orig_console_fns.warn;
                    break;
                }
                case "ERROR": {
                    emit = severityText === "ERROR";
                    emitter = this.orig_console_fns.error;
                    break;
                }
            }

            if (emit) {
                if (this.console.custom_out) {
                    emitter = this.console.custom_out;
                }
                if (attributes !== undefined) {
                    emitter(message, attributes);
                } else {
                    emitter(message);
                }
            }
        }

        // Emit to oltp if log level meets level_from:
        let emitOltp = false;
        switch (this.oltp.level_from) {
            case "DEBUG": {
                emitOltp = true;
                break;
            }
            case "INFO": {
                emitOltp = severityText !== "DEBUG";
                break;
            }
            case "WARN": {
                emitOltp = severityText === "WARN" || severityText === "ERROR";
                break;
            }
            case "ERROR": {
                emitOltp = severityText === "ERROR";
                break;
            }
        }
        if (emitOltp) {
            this.logger.emit({
                severityText,
                body: message,
                attributes,
            });
        }
    }

    _set_console_fns() {
        this.orig_console_fns = {
            log: console.log,
            debug: console.debug,
            info: console.info,
            warn: console.warn,
            error: console.error,
        };
        console.log = (msg, ...attrs) => this.info(msg, ...attrs);
        console.debug = (msg, ...attrs) => this.debug(msg, ...attrs);
        console.info = (msg, ...attrs) => this.info(msg, ...attrs);
        console.warn = (msg, ...attrs) => this.warn(msg, ...attrs);
        console.error = (msg, ...attrs) => this.error(msg, ...attrs);
    }

    /** Create the global logger, must setup oltp (http), console can be optionally setup and will just print logs. */
    constructor({
        otlp,
        console = undefined,
    }: {
        console?: ConsoleArgs;
        otlp: OltpArgs;
    }) {
        this.console = console ? console : null;
        this.oltp = otlp;
        // Store original console fns and override with those in this global logger:
        this._set_console_fns();

        const resource = new Resource({
            [SemanticResourceAttributes.SERVICE_NAME]: otlp.service_name,
            [SemanticResourceAttributes.SERVICE_VERSION]: otlp.service_version,
        });

        // Url will be added for each usage, different for traces/logs/metrics
        const baseExporterConfig: OTLPExporterNodeConfigBase = {
            keepAlive: true,
        };
        const bufferConfig: BufferConfig = {
            maxQueueSize: 2048,
            maxExportBatchSize: 512,
            scheduledDelayMillis: 5000,
            exportTimeoutMillis: 30000,
        };

        this.meterProvider = new MeterProvider({
            resource,
            readers: [
                new PeriodicExportingMetricReader({
                    exporter: new OTLPMetricExporter({
                        ...baseExporterConfig,
                        url: `${otlp.endpoint}/v1/metrics`,
                    }),
                    exportIntervalMillis: 60000, // Haven't found a way for it to not send when no metrics yet, so changing from 1s to 60s to not bloat the network logs of a client.
                }),
            ],
        });

        this.loggerProvider = new LoggerProvider({
            resource,
        });
        this.loggerProvider.addLogRecordProcessor(
            new BatchLogRecordProcessor(
                new OTLPLogExporter({
                    ...baseExporterConfig,
                    url: `${otlp.endpoint}/v1/logs`,
                }),
            ),
        );
        this.logger = this.loggerProvider.getLogger("GlobalLog");

        this.tracerProvider = new WebTracerProvider({ resource });
        this.tracerProvider.addSpanProcessor(
            new BatchSpanProcessor(
                new OTLPTraceExporter({
                    ...baseExporterConfig,
                    url: `${otlp.endpoint}/v1/traces`,
                }),
                bufferConfig,
            ),
        );
        // Enable auto-context propagation within the application using zones:
        this.tracerProvider.register({
            contextManager: new ZoneContextManager(),
            // Configure the propagator to enable context propagation between services,
            // uses the W3C Trace Headers (traceparent, tracestate) and W3C Baggage Headers (baggage).
            propagator: new CompositePropagator({
                propagators: [new W3CBaggagePropagator(), new W3CTraceContextPropagator()],
            }),
        });
        this.tracer = this.tracerProvider.getTracer("GlobalLog");

        registerInstrumentations({
            instrumentations: [
                // Trace user site interactions:
                new UserInteractionInstrumentation({}),
                // Trace client document loading:
                new DocumentLoadInstrumentation({}),
                // Auto instrument fetch requests:
                new FetchInstrumentation({}),
            ],
        });

        // Register it as the current global logger:
        _LOG = this;
    }
}

/* The global accessor for logging, will use the active global logger: */
export const LOG = new Proxy(GlobalLog.prototype, {
    get: (target, property, receiver) => {
        if (_LOG) {
            return Reflect.get(_LOG, property, receiver);
        }
        throw new Error("Global log not yet initialized!");
    },
});

export { GlobalLog };
