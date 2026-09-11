/**
 * Memory events as data. Storage format is a Codec; identity is a Canonicalizer.
 * Import `./json.ts` only at the edges that actually speak JSON.
 * Envelope helpers and isOpenAction do not depend on JSON.
 */
export type { FromKind, Kind, MemoryEvent } from "./types.ts";
export { assertEvent } from "./types.ts";
export type { Bytes, Canonicalizer, Codec } from "./codec.ts";
export { bytesEqual, eventEqual } from "./codec.ts";
export { isOpenAction, kindOf, refsOf, sessionOf } from "./envelope.ts";
