import { hold } from "./json.ts";
import type { Dict, JsonValue } from "./types.ts";

export const dict = {
  of: <T extends JsonValue>(entries: Dict<T> = {}): Dict<T> => hold(entries) as Dict<T>,

  get: <T extends JsonValue>(from: Dict<T>, key: string): T | undefined => from[key],

  has: (from: Dict<JsonValue>, key: string): boolean => Object.hasOwn(from, key),

  set: <T extends JsonValue>(from: Dict<T>, key: string, value: T): Dict<T> =>
    hold({ ...from, [key]: value }) as Dict<T>,

  remove: <T extends JsonValue>(from: Dict<T>, key: string): Dict<T> => {
    const next: Record<string, T> = { ...from };
    delete next[key];
    return hold(next) as Dict<T>;
  },

  keys: (from: Dict<JsonValue>): readonly string[] => hold(Object.keys(from)),
};
