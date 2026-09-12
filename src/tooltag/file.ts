import { isDownward, isSamePath, normalizePath } from "./path.ts";
import type { FileFacet } from "./types.ts";

const writes = (facet: FileFacet): boolean => facet.op === "w" || facet.op === "rw";

export const fileCoversPath = (facet: FileFacet, path: string): boolean => {
  if (facet.op === "none") return false;
  if (facet.op === "unbounded") return true;
  if (isSamePath(facet.path, path)) return true;
  return facet.recursive === true && isDownward(facet.path, path);
};

const filePathsOverlap = (left: FileFacet, right: FileFacet): boolean => {
  if (left.op === "none" || right.op === "none") return false;
  if (left.op === "unbounded" || right.op === "unbounded") return true;
  if (left.op !== "r" && left.op !== "w" && left.op !== "rw") return false;
  if (right.op !== "r" && right.op !== "w" && right.op !== "rw") return false;
  const a = normalizePath(left.path);
  const b = normalizePath(right.path);
  if (a === b) return true;
  if (left.recursive === true && isDownward(a, b)) return true;
  if (right.recursive === true && isDownward(b, a)) return true;
  return false;
};

/** Parallel-session conflict from the file face only. Permit and memory do not lock. */
export const fileConflicts = (left: FileFacet, right: FileFacet): boolean => {
  if (left.op === "none" || right.op === "none") return false;
  if (left.op === "unbounded" || right.op === "unbounded") return true;
  if (!writes(left) && !writes(right)) return false;
  return filePathsOverlap(left, right);
};
