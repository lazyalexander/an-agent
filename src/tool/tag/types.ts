export type FileOp = "none" | "r" | "w" | "rw" | "unbounded";

export type Permit = "ask" | "forbidden" | "go";

export type MemoryOp = "remember" | "forget" | "ignore";

export type FileFacet =
  | { readonly op: "none" }
  | { readonly op: "unbounded" }
  | { readonly op: "r" | "w" | "rw"; readonly path: string; readonly recursive?: boolean };

export type MemoryFacet =
  | { readonly op: "ignore" }
  | { readonly op: "remember"; readonly aspect?: string }
  | { readonly op: "forget"; readonly rememberId: string };

/** One invocation by one worker. Three orthogonal faces; scope sits on the facets. */
export type ToolTag = {
  readonly file: FileFacet;
  readonly permit: Permit;
  readonly memory: MemoryFacet;
};

/** Closed memevent tag: remember ingest. Forget uses `forget` so the fold can find it. */
export const OUTSIDE_TAG = "outside";
export const FORGET_TAG = "forget";
