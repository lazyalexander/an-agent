import { appendFileSync, existsSync, mkdirSync, readFileSync } from "node:fs";
import { dirname } from "node:path";
import type { MemoryRecord, MemoryStore } from "./types.ts";
import { ulid } from "./ulid.ts";

const parseLine = (line: string): MemoryRecord | undefined => {
  if (line.trim() === "") return undefined;
  return JSON.parse(line) as MemoryRecord;
};

export function createJsonlMemoryStore(path: string): MemoryStore {
  mkdirSync(dirname(path), { recursive: true });
  // Cached after the first scan so append does not reread the whole file.
  let nextSeq: number | undefined;

  const readAll = (): MemoryRecord[] => {
    if (!existsSync(path)) return [];
    return readFileSync(path, "utf8").split("\n").flatMap((line) => {
      const record = parseLine(line);
      return record ? [record] : [];
    });
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
      const record: MemoryRecord = {
        v: 1,
        id: ulid(),
        seq,
        ts: new Date().toISOString(),
        ...input,
      };
      appendFileSync(path, `${JSON.stringify(record)}\n`);
      nextSeq = seq + 1;
      return record;
    },
    readAll,
  };
}
