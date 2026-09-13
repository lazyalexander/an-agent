import type { MemoryEvent } from "../bedrock/memkit/types.ts";

export type { FromKind, Kind, MemoryEvent } from "../bedrock/memkit/types.ts";

/** A memevent on this agent's memstream. Alias of MemoryEvent. */
export type MemoryRecord = MemoryEvent;

export type MemoryInput = Omit<MemoryRecord, "v" | "id" | "seq" | "ts" | "session"> & {
  session: string;
};

export type MemoryStore = {
  append: (input: MemoryInput) => MemoryRecord;
  readAll: () => readonly MemoryRecord[];
};
