import { assertEvent, type MemoryEvent } from "./types.ts";
import type { Canonicalizer, Codec } from "./codec.ts";

const utf8 = new TextEncoder();
const utf8In = new TextDecoder();

const sortJson = (value: unknown): unknown => {
  if (value === null || typeof value !== "object") return value;
  if (Array.isArray(value)) return value.map(sortJson);
  const source = value as Record<string, unknown>;
  const out: Record<string, unknown> = {};
  for (const key of Object.keys(source).sort()) out[key] = sortJson(source[key]);
  return out;
};

/** Canonical JSON bytes for hashing and equality. Swap this with the carrier. */
export const jsonCanonical: Canonicalizer = (event) =>
  utf8.encode(JSON.stringify(sortJson(event)));

export const jsonCodec: Codec = {
  encode: (event) => utf8.encode(JSON.stringify(event)),
  decode: (bytes) => assertEvent(JSON.parse(utf8In.decode(bytes))),
};

export const jsonLine = {
  encode: (event: MemoryEvent): string => `${JSON.stringify(event)}\n`,
  decode: (line: string): MemoryEvent => assertEvent(JSON.parse(line)),
};
