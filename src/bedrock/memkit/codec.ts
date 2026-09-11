import type { MemoryEvent } from "./types.ts";

/** One event as opaque bytes. The tape format is a Codec, not the event type. */
export type Bytes = Uint8Array;

export type Codec = {
  encode: (event: MemoryEvent) => Bytes;
  decode: (bytes: Bytes) => MemoryEvent;
};

/** Stable bytes for equality and hashing. May differ from the stored encoding. */
export type Canonicalizer = (event: MemoryEvent) => Bytes;

export const bytesEqual = (left: Bytes, right: Bytes): boolean => {
  if (left.byteLength !== right.byteLength) return false;
  for (let i = 0; i < left.byteLength; i += 1) {
    if (left[i] !== right[i]) return false;
  }
  return true;
};

export const eventEqual = (
  left: MemoryEvent,
  right: MemoryEvent,
  canonical: Canonicalizer,
): boolean => bytesEqual(canonical(left), canonical(right));
