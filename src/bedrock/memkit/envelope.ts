import type { MemoryEvent } from "./types.ts";

export const kindOf = (event: MemoryEvent): MemoryEvent["kind"] => event.kind;

export const sessionOf = (event: MemoryEvent): string | undefined => event.session;

export const refsOf = (event: MemoryEvent): readonly string[] => event.refs;

/** True when no observation points at this action id. That is not success. */
export const isOpenAction = (events: readonly MemoryEvent[], actionId: string): boolean =>
  !events.some((event) => event.kind === "observation" && event.refs.includes(actionId));
