import { isDownward, isSamePath } from "./path.ts";
import type { Permit } from "./types.ts";

export type PermitRule = {
  readonly path: string;
  readonly recursive?: boolean;
  readonly permit: Permit;
};

/**
 * How a parent-path rule bears on `path`.
 * Same path: the rule applies as written.
 * Descendant: only if recursive; forbidden and ask close downward; go does not transfer.
 */
export const permitImplied = (rule: PermitRule, path: string): Permit | undefined => {
  if (isSamePath(rule.path, path)) return rule.permit;
  if (rule.recursive !== true || !isDownward(rule.path, path)) return undefined;
  if (rule.permit === "go") return undefined;
  return rule.permit;
};
