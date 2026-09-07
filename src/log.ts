import { appendFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";

export type LogEvent = {
  type: string;
  ts?: string;
  [key: string]: unknown;
};

export type Log = {
  // Append-only. There is no truncate/rewrite API on purpose.
  append: (event: LogEvent) => void;
};

export const appendLog = (log: Log, event: LogEvent): void => {
  log.append({ ts: new Date().toISOString(), ...event });
};

export const createFileLog = (path: string): Log => {
  mkdirSync(dirname(path), { recursive: true });
  return {
    append: (event) => {
      const record = { ts: new Date().toISOString(), ...event };
      appendFileSync(path, `${JSON.stringify(record)}\n`);
    },
  };
};
