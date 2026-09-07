import { appendFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import type { EnvMap } from "./load-env.ts";

export type OpsEvent = {
  ts: string;
  level: "info" | "error";
  kind: "latency" | "error";
  name: string;
  ms?: number;
  error?: string;
};

export type Ops = {
  record: (event: Omit<OpsEvent, "ts">) => void;
};

export const noopOps: Ops = { record: () => {} };

export function createFileOps(path: string): Ops {
  mkdirSync(dirname(path), { recursive: true });
  return {
    record: (event) => {
      const line = JSON.stringify({ ts: new Date().toISOString(), ...event });
      appendFileSync(path, `${line}\n`);
    },
  };
}

const otlpUrl = (endpoint: string): string => {
  const base = endpoint.replace(/\/$/, "");
  return base.endsWith("/v1/logs") ? base : `${base}/v1/logs`;
};

const attr = (key: string, value: string | number) =>
  typeof value === "number"
    ? { key, value: { intValue: String(Math.trunc(value)) } }
    : { key, value: { stringValue: value } };

export function createOtlpOps(options: { endpoint: string; fetch?: typeof fetch }): Ops {
  const url = otlpUrl(options.endpoint);
  const fetchImpl = options.fetch ?? fetch;
  return {
    record: (event) => {
      const attributes = [
        attr("kind", event.kind),
        attr("level", event.level),
        ...(event.ms !== undefined ? [attr("ms", event.ms)] : []),
        ...(event.error ? [attr("error", event.error)] : []),
      ];
      const payload = {
        resourceLogs: [
          {
            resource: {
              attributes: [attr("service.name", "an-agent")],
            },
            scopeLogs: [
              {
                scope: { name: "an-agent" },
                logRecords: [
                  {
                    timeUnixNano: `${Date.now()}000000`,
                    severityText: event.level === "error" ? "ERROR" : "INFO",
                    body: { stringValue: event.name },
                    attributes,
                  },
                ],
              },
            ],
          },
        ],
      };
      void fetchImpl(url, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(payload),
      }).catch(() => {
        /* export must not take down the agent */
      });
    },
  };
}

export function composeOps(sinks: readonly Ops[]): Ops {
  if (sinks.length === 0) return noopOps;
  if (sinks.length === 1) return sinks[0]!;
  return {
    record: (event) => {
      for (const sink of sinks) sink.record(event);
    },
  };
}

export function resolveOps(options: {
  debugPath?: string;
  env?: EnvMap;
  fetch?: typeof fetch;
}): Ops {
  const env = options.env ?? process.env;
  const sinks: Ops[] = [];
  if (options.debugPath) sinks.push(createFileOps(options.debugPath));
  const mode = (env.AN_AGENT_OPS ?? "").toLowerCase();
  if (mode === "otlp") {
    const endpoint = env.OTEL_EXPORTER_OTLP_ENDPOINT;
    if (!endpoint) throw new Error("OTEL_EXPORTER_OTLP_ENDPOINT is required when AN_AGENT_OPS=otlp");
    sinks.push(createOtlpOps({ endpoint, fetch: options.fetch }));
  }
  return composeOps(sinks);
}

export async function timed<T>(ops: Ops, name: string, fn: () => Promise<T>): Promise<T> {
  const started = Date.now();
  try {
    const value = await fn();
    ops.record({ kind: "latency", level: "info", name, ms: Date.now() - started });
    return value;
  } catch (err) {
    ops.record({
      kind: "error",
      level: "error",
      name,
      ms: Date.now() - started,
      error: err instanceof Error ? err.message : String(err),
    });
    throw err;
  }
}
