/**
 * Act is the only writer of memstream. Tool calls are one kind of act.
 * Tags live on the envelope; Effect is sensed from tags, not from Tool.
 */
export type { ActEnvelope, ActKind, ActRecord, Effect } from "./types.ts";
export { effectFromTag } from "./sense.ts";
export { admitAct, admitAgentAct, admitUtterance } from "./admit.ts";
export type { ToolActResult } from "./tool.ts";
export { runToolAct } from "./tool.ts";
export type {
  FileFacet,
  FileOp,
  MemoryFacet,
  MemoryOp,
  Permit,
  PermitRule,
  ToolTag,
} from "./tag/index.ts";
export {
  FORGET_TAG,
  OUTSIDE_TAG,
  fileConflicts,
  fileCoversPath,
  forgottenRememberIds,
  isDownward,
  isForget,
  isMaskedOutside,
  isOutside,
  isSamePath,
  normalizePath,
  permitImplied,
  toolTag,
} from "./tag/index.ts";
