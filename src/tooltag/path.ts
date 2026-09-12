/** Slash-normalized workplace path. Does not case-fold. */
export const normalizePath = (path: string): string => {
  const unified = path.replaceAll("\\", "/").replace(/\/{2,}/g, "/");
  if (unified.length > 1 && unified.endsWith("/")) return unified.slice(0, -1);
  return unified === "" ? "/" : unified;
};

export const isSamePath = (left: string, right: string): boolean =>
  normalizePath(left) === normalizePath(right);

/** True when `child` is `parent` or a descendant. */
export const isDownward = (parent: string, child: string): boolean => {
  const p = normalizePath(parent);
  const c = normalizePath(child);
  if (p === c) return true;
  const prefix = p === "/" ? "/" : `${p}/`;
  return c.startsWith(prefix);
};
