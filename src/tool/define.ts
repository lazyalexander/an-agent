import { toolTag } from "./tag/make.ts";
import type { FileFacet, FileOp, MemoryFacet, MemoryOp, Permit, ToolTag } from "./tag/types.ts";
import type { Tool } from "./core.ts";

export type TaggedTool<T extends ToolTag = ToolTag> = Tool & { readonly tag: T };

export type TagPatch = {
  path?: string;
  recursive?: boolean;
  fileOp?: FileOp;
  permit?: Permit;
  memoryOp?: MemoryOp;
  aspect?: string;
  rememberId?: string;
};

const FILE_RANK: Record<FileOp, number> = {
  none: 0,
  r: 1,
  w: 2,
  rw: 3,
  unbounded: 4,
};

const PERMIT_TIGHTNESS: Record<Permit, number> = {
  go: 0,
  ask: 1,
  forbidden: 2,
};

export const isTaggedTool = (tool: Tool): tool is TaggedTool =>
  "tag" in tool && typeof (tool as TaggedTool).tag === "object" && (tool as TaggedTool).tag !== null;

/** Untagged tools are treated as unbounded workplace access. */
export const tagOf = (tool: Tool): ToolTag =>
  isTaggedTool(tool) ? tool.tag : toolTag.unbounded();

export function defineTool<T extends ToolTag>(def: Tool & { tag: T }): TaggedTool<T> {
  return def;
}

const resolveFile = (declared: FileFacet, patch: TagPatch): FileFacet => {
  if (patch.fileOp !== undefined && FILE_RANK[patch.fileOp] > FILE_RANK[declared.op]) {
    throw new Error(`cannot widen file face from ${declared.op} to ${patch.fileOp}`);
  }
  if (declared.op === "none") return declared;
  if (declared.op === "unbounded") {
    const op = patch.fileOp ?? "unbounded";
    if (op === "none" || op === "unbounded") return { op };
    if (!patch.path) throw new Error("path is required to narrow unbounded");
    return { op, path: patch.path, recursive: patch.recursive };
  }
  return {
    op: declared.op,
    path: patch.path ?? declared.path,
    recursive: patch.recursive ?? declared.recursive,
  };
};

const resolvePermit = (declared: Permit, patch: TagPatch): Permit => {
  if (patch.permit === undefined) return declared;
  if (PERMIT_TIGHTNESS[patch.permit] < PERMIT_TIGHTNESS[declared]) {
    throw new Error(`cannot widen permit from ${declared} to ${patch.permit}`);
  }
  return patch.permit;
};

const resolveMemory = (declared: MemoryFacet, patch: TagPatch): MemoryFacet => {
  const next = patch.memoryOp ?? declared.op;
  if (declared.op === "ignore") {
    if (next === "ignore") return declared;
    if (next === "remember") {
      return patch.aspect === undefined ? { op: "remember" } : { op: "remember", aspect: patch.aspect };
    }
    throw new Error("cannot change memory face from ignore to forget");
  }
  if (declared.op === "remember") {
    if (next !== "remember") throw new Error(`cannot change memory face from remember to ${next}`);
    return patch.aspect === undefined
      ? declared
      : { op: "remember", aspect: patch.aspect };
  }
  if (next !== "forget") throw new Error(`cannot change memory face from forget to ${next}`);
  return { op: "forget", rememberId: patch.rememberId ?? declared.rememberId };
};

/** Fill path / aspect / rememberId without widening the declared faces. */
export const resolveTag = (declared: ToolTag, patch: TagPatch = {}): ToolTag => ({
  file: resolveFile(declared.file, patch),
  permit: resolvePermit(declared.permit, patch),
  memory: resolveMemory(declared.memory, patch),
});
