import { appendFileSync, existsSync, mkdirSync, readFileSync } from "node:fs";
import { dirname } from "node:path";
import { ulid } from "./ulid.ts";

export type Kind = "utterance" | "action" | "observation";
export type Tag = Kind;
export type FromKind = "human" | "agent";

export type MemoryRecord = {
  v: 1;
  id: string;
  seq: number;
  ts: string;
  from: string;
  from_kind: FromKind;
  kind: Kind;
  tags: readonly Tag[];
  content: string;
  refs: readonly string[];
};

export type MemoryInput = Omit<MemoryRecord, "v" | "id" | "seq" | "ts">;

export type MemoryStore = {
  append: (input: MemoryInput) => MemoryRecord;
  readAll: () => readonly MemoryRecord[];
};

const parseLine = (line: string): MemoryRecord | undefined => {
  if (line.trim() === "") return undefined;
  return JSON.parse(line) as MemoryRecord;
};

export function createJsonlMemoryStore(path: string): MemoryStore {
  mkdirSync(dirname(path), { recursive: true });
  const readAll = (): MemoryRecord[] => {
    if (!existsSync(path)) return [];
    return readFileSync(path, "utf8").split("\n").flatMap((line) => {
      const record = parseLine(line);
      return record ? [record] : [];
    });
  };
  return {
    append: (input) => {
      const record: MemoryRecord = {
        v: 1,
        id: ulid(),
        seq: readAll().length + 1,
        ts: new Date().toISOString(),
        ...input,
      };
      appendFileSync(path, `${JSON.stringify(record)}\n`);
      return record;
    },
    readAll,
  };
}
