import { appendFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";

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

export function createFileOps(path: string): Ops {
  mkdirSync(dirname(path), { recursive: true });
  return {
    record: (event) => {
      const line = JSON.stringify({ ts: new Date().toISOString(), ...event });
      appendFileSync(path, `${line}\n`);
    },
  };
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
