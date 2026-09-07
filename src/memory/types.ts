export type Kind = "utterance" | "action" | "observation";

/** Declared by the ingress channel. Without a protocol tag this is "unknown". */
export type FromKind = "human" | "agent" | "unknown";

/**
 * Closed enum. None are defined yet, so writers pass [].
 * Session is a record field, not a tag.
 */
export type Tag = never;

/**
 * One event on this agent's tape. The tape is append-only testimony:
 * it records what this agent experienced, not a shared conversation object.
 * `session` groups events; membership and collaboration live elsewhere.
 * Older files may omit `session`; new appends always write one.
 */
export type MemoryRecord = {
  v: 1;
  id: string;
  seq: number;
  ts: string;
  from: string;
  from_kind: FromKind;
  kind: Kind;
  session?: string;
  content: string;
  tags: readonly Tag[];
  refs: readonly string[];
};

export type MemoryInput = Omit<MemoryRecord, "v" | "id" | "seq" | "ts" | "session"> & {
  session: string;
};

export type MemoryStore = {
  append: (input: MemoryInput) => MemoryRecord;
  readAll: () => readonly MemoryRecord[];
};
