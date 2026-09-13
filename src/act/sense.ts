import type { Effect } from "./types.ts";
import type { ToolTag } from "./tag/types.ts";

/** Derive workplace effect from act tags. Not declared by the tool body. */
export const effectFromTag = (tag: ToolTag, workplace?: string): Effect => {
  const file = tag.file;
  if (file.op === "none") {
    return { reads: [], writes: [], unbounded: false, workplace, memory: tag.memory.op };
  }
  if (file.op === "unbounded") {
    return { reads: [], writes: [], unbounded: true, workplace, memory: tag.memory.op };
  }
  const path = file.path;
  const reads = file.op === "r" || file.op === "rw" ? [path] : [];
  const writes = file.op === "w" || file.op === "rw" ? [path] : [];
  return { reads, writes, unbounded: false, workplace, memory: tag.memory.op };
};
