import { isNullable } from "./types.ts";

/** True when every item in `needles` appears in `haystack`. */
export function contain<T>(haystack: readonly T[], needles: readonly T[]): boolean {
  return needles.every((item) => haystack.includes(item));
}

export function intersection<T>(left: readonly T[], right: readonly T[]): T[] {
  return left.filter((item) => right.includes(item));
}

export function difference<T>(left: readonly T[], right: readonly T[]): T[] {
  return left.filter((item) => !right.includes(item));
}

export function union<T>(left: readonly T[], right: readonly T[]): T[] {
  return [...new Set([...left, ...right])];
}

export function deduplicate<T>(items: readonly T[]): T[] {
  return [...new Set(items)];
}

/** New array without the first matching item. Does not splice in place. */
export function without<T>(items: readonly T[], item: T): T[] {
  const index = items.indexOf(item);
  if (index < 0) return [...items];
  return [...items.slice(0, index), ...items.slice(index + 1)];
}

export function makeArray<T>(source: T | T[] | null | undefined): T[] {
  if (Array.isArray(source)) return [...source];
  return isNullable(source) ? [] : [source];
}
