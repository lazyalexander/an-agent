export type Kind = "utterance" | "action" | "observation";

export type FromKind = "human" | "agent" | "unknown";

/**
 * One memory event as data. Independent of how the tape is stored.
 * Carriers implement Codec; they do not add fields here.
 */
export type MemoryEvent = {
  v: 1;
  id: string;
  seq: number;
  ts: string;
  from: string;
  from_kind: FromKind;
  kind: Kind;
  session?: string;
  content: string;
  tags: readonly string[];
  refs: readonly string[];
};

const KINDS: readonly Kind[] = ["utterance", "action", "observation"];
const FROM_KINDS: readonly FromKind[] = ["human", "agent", "unknown"];

const isKind = (value: unknown): value is Kind =>
  typeof value === "string" && (KINDS as readonly string[]).includes(value);

const isFromKind = (value: unknown): value is FromKind =>
  typeof value === "string" && (FROM_KINDS as readonly string[]).includes(value);

const isStringArray = (value: unknown): value is string[] =>
  Array.isArray(value) && value.every((item) => typeof item === "string");

/** Structural check. Does not parse a carrier. */
export function assertEvent(value: unknown): MemoryEvent {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("memory event must be an object");
  }
  const row = value as Record<string, unknown>;
  if (row.v !== 1) throw new Error("memory event v must be 1");
  if (typeof row.id !== "string") throw new Error("memory event id must be a string");
  if (typeof row.seq !== "number" || !Number.isFinite(row.seq)) {
    throw new Error("memory event seq must be a finite number");
  }
  if (typeof row.ts !== "string") throw new Error("memory event ts must be a string");
  if (typeof row.from !== "string") throw new Error("memory event from must be a string");
  if (!isFromKind(row.from_kind)) throw new Error("memory event from_kind is invalid");
  if (!isKind(row.kind)) throw new Error("memory event kind is invalid");
  if (row.session !== undefined && typeof row.session !== "string") {
    throw new Error("memory event session must be a string when present");
  }
  if (typeof row.content !== "string") throw new Error("memory event content must be a string");
  if (!isStringArray(row.tags)) throw new Error("memory event tags must be a string array");
  if (!isStringArray(row.refs)) throw new Error("memory event refs must be a string array");
  return row as MemoryEvent;
}
