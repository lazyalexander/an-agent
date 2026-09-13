import { appendFileSync, existsSync, mkdirSync, readFileSync } from "node:fs";
import { dirname } from "node:path";
import { jsonLine } from "../bedrock/memkit/json.ts";
import { assertEvent } from "../bedrock/memkit/types.ts";
import { Time } from "../bedrock/opkit/time.ts";
import type { MemoryRecord, MemoryStore } from "./types.ts";
import { ulid } from "./ulid.ts";

export type JsonlMemoryOptions = {
  /** Milliseconds since epoch. Defaults to Date.now. */
  now?: () => number;
};

export function createJsonlMemoryStore(
  path: string,
  options: JsonlMemoryOptions = {},
): MemoryStore {
  mkdirSync(dirname(path), { recursive: true });
  const now = options.now ?? Date.now;
  // Cached after the first scan so append does not reread the whole file.
  let nextSeq: number | undefined;

  const readAll = (): MemoryRecord[] => {
    if (!existsSync(path)) return [];
    const records: MemoryRecord[] = [];
    for (const line of readFileSync(path, "utf8").split("\n")) {
      if (line.trim() === "") continue;
      records.push(jsonLine.decode(line));
    }
    return records;
  };

  const peekNextSeq = (): number => {
    if (nextSeq !== undefined) return nextSeq;
    const last = readAll().at(-1);
    nextSeq = (last?.seq ?? 0) + 1;
    return nextSeq;
  };

  return {
    append: (input) => {
      const seq = peekNextSeq();
      const record = assertEvent({
        v: 1,
        id: ulid(),
        seq,
        ts: Time.iso(now()),
        ...input,
      });
      appendFileSync(path, jsonLine.encode(record));
      nextSeq = seq + 1;
      return record;
    },
    readAll,
  };
}
