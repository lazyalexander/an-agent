import type { FileFacet, MemoryFacet, Permit, ToolTag } from "./types.ts";

const tag = (file: FileFacet, permit: Permit, memory: MemoryFacet): ToolTag => ({
  file,
  permit,
  memory,
});

export const toolTag = {
  of: (value: ToolTag): ToolTag => value,

  /** Pure compute: no workplace, no ingest. */
  none: (permit: Permit = "go"): ToolTag =>
    tag({ op: "none" }, permit, { op: "ignore" }),

  read: (path: string, extra: { recursive?: boolean; permit?: Permit } = {}): ToolTag =>
    tag(
      { op: "r", path, recursive: extra.recursive },
      extra.permit ?? "go",
      { op: "ignore" },
    ),

  write: (path: string, extra: { recursive?: boolean; permit?: Permit } = {}): ToolTag =>
    tag(
      { op: "w", path, recursive: extra.recursive },
      extra.permit ?? "ask",
      { op: "ignore" },
    ),

  readWrite: (path: string, extra: { recursive?: boolean; permit?: Permit } = {}): ToolTag =>
    tag(
      { op: "rw", path, recursive: extra.recursive },
      extra.permit ?? "ask",
      { op: "ignore" },
    ),

  unbounded: (permit: Permit = "ask"): ToolTag =>
    tag({ op: "unbounded" }, permit, { op: "ignore" }),

  remember: (aspect?: string, permit: Permit = "go"): ToolTag =>
    tag({ op: "none" }, permit, aspect === undefined ? { op: "remember" } : { op: "remember", aspect }),

  forget: (rememberId: string, permit: Permit = "go"): ToolTag =>
    tag({ op: "none" }, permit, { op: "forget", rememberId }),
};
