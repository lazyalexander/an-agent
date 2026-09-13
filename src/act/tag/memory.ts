import type { MemoryEvent } from "../../bedrock/memkit/types.ts";
import { FORGET_TAG, OUTSIDE_TAG } from "./types.ts";

export const isOutside = (event: MemoryEvent): boolean => event.tags.includes(OUTSIDE_TAG);

export const isForget = (event: MemoryEvent): boolean => event.tags.includes(FORGET_TAG);

/** Remember ids that have a forget memevent pointing at them. */
export const forgottenRememberIds = (events: readonly MemoryEvent[]): ReadonlySet<string> => {
  const ids = new Set<string>();
  for (const event of events) {
    if (!isForget(event)) continue;
    for (const id of event.refs) ids.add(id);
  }
  return ids;
};

const byId = (events: readonly MemoryEvent[]): Map<string, MemoryEvent> => {
  const map = new Map<string, MemoryEvent>();
  for (const event of events) map.set(event.id, event);
  return map;
};

/**
 * An `outside` memevent is masked if its remember id was forgotten, or it
 * refs a masked outside memevent. Non-outside events are not masked.
 */
export const isMaskedOutside = (
  event: MemoryEvent,
  events: readonly MemoryEvent[],
): boolean => {
  if (!isOutside(event)) return false;
  const forgotten = forgottenRememberIds(events);
  const index = byId(events);
  const walk = (id: string, seen: Set<string>): boolean => {
    if (forgotten.has(id)) return true;
    const node = index.get(id);
    if (!node || !isOutside(node)) return false;
    if (seen.has(id)) return false;
    seen.add(id);
    return node.refs.some((ref) => walk(ref, seen));
  };
  return walk(event.id, new Set());
};
