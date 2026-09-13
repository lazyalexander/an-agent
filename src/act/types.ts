import type { MemoryOp, ToolTag } from "./tag/types.ts";

export type ActKind =
  | "utterance"
  | "tool"
  | "mount"
  | "unmount"
  | "forbid"
  | "allow"
  | "remember"
  | "forget";

/** Faces for this act. Authority for memstream; not the Tool type. */
export type ActEnvelope = {
  readonly kind: ActKind;
  readonly tag: ToolTag;
  readonly workplace?: string;
  readonly tool?: string;
};

/** Sensed from the envelope tag. Tools do not author this. */
export type Effect = {
  readonly reads: readonly string[];
  readonly writes: readonly string[];
  readonly unbounded: boolean;
  readonly workplace?: string;
  readonly memory: MemoryOp;
};

export type ActRecord = ActEnvelope & {
  readonly effect?: Effect;
};
