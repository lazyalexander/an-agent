/**
 * Orthogonal faces on one worker's tool call: file, permit, memory.
 * They do not fill in each other's meaning. Workplace concurrency uses file
 * conflict; memstream visibility uses forget refs, not a mutable field.
 */
export type { FileFacet, FileOp, MemoryFacet, MemoryOp, Permit, ToolTag } from "./types.ts";
export { FORGET_TAG, OUTSIDE_TAG } from "./types.ts";
export { normalizePath, isDownward, isSamePath } from "./path.ts";
export { fileConflicts, fileCoversPath } from "./file.ts";
export type { PermitRule } from "./permit.ts";
export { permitImplied } from "./permit.ts";
export { forgottenRememberIds, isForget, isMaskedOutside, isOutside } from "./memory.ts";
export { toolTag } from "./tag.ts";
