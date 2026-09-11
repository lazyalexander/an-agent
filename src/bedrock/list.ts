import { hold } from "./json.ts";
import type { JsonValue } from "./types.ts";

export const list = {
  of: <T extends JsonValue>(...items: T[]): readonly T[] => hold(items) as readonly T[],

  append: <T extends JsonValue>(items: readonly T[], item: T): readonly T[] =>
    hold([...items, item]) as readonly T[],

  concat: <T extends JsonValue>(left: readonly T[], right: readonly T[]): readonly T[] =>
    hold([...left, ...right]) as readonly T[],

  at: <T extends JsonValue>(items: readonly T[], index: number): T | undefined => items[index],

  slice: <T extends JsonValue>(items: readonly T[], start?: number, end?: number): readonly T[] =>
    hold(items.slice(start, end)) as readonly T[],
};
