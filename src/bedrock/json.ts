import type { JsonObject, JsonValue } from "./types.ts";

const isPlainObject = (value: object): boolean => {
  const proto = Object.getPrototypeOf(value);
  return proto === Object.prototype || proto === null;
};

export const isJsonValue = (value: unknown): value is JsonValue => {
  if (value === null) return true;
  if (typeof value === "string" || typeof value === "boolean") return true;
  if (typeof value === "number") return Number.isFinite(value);
  if (typeof value !== "object") return false;
  if (Array.isArray(value)) return value.every(isJsonValue);
  if (!isPlainObject(value)) return false;
  const record = value as Record<string, unknown>;
  return Object.keys(record).every((key) => isJsonValue(record[key]));
};

const assertJson = (value: unknown, label: string): JsonValue => {
  if (!isJsonValue(value)) {
    throw new Error(`${label} must be JSON-safe (no undefined, functions, class instances, or NaN)`);
  }
  return value;
};

const cloneJson = (value: JsonValue): JsonValue => {
  if (value === null || typeof value !== "object") return value;
  if (Array.isArray(value)) return value.map(cloneJson);
  const object = value as JsonObject;
  const out: Record<string, JsonValue> = {};
  for (const key of Object.keys(object)) out[key] = cloneJson(object[key]);
  return out;
};

const freezeJson = (value: JsonValue): JsonValue => {
  if (value === null || typeof value !== "object" || Object.isFrozen(value)) return value;
  if (Array.isArray(value)) {
    for (const item of value) freezeJson(item);
  } else {
    const object = value as JsonObject;
    for (const key of Object.keys(object)) freezeJson(object[key]);
  }
  return Object.freeze(value);
};

/** Deep copy. Does not freeze and does not mutate the input. */
export const clone = <T extends JsonValue>(value: T): T => {
  assertJson(value, "clone");
  return cloneJson(value) as T;
};

/**
 * Snapshot a value into a frozen tree. The input is left untouched.
 * Later mutations throw in this module's strict ESM runtime.
 */
export const hold = <T extends JsonValue>(value: T): T => {
  assertJson(value, "hold");
  return freezeJson(cloneJson(value)) as T;
};

export const equal = (left: unknown, right: unknown): boolean => {
  const a = assertJson(left, "equal");
  const b = assertJson(right, "equal");
  if (a === b) return true;
  if (a === null || b === null || typeof a !== "object" || typeof b !== "object") return false;
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) return false;
    return a.every((item, i) => equal(item, b[i]));
  }
  const objectA = a as JsonObject;
  const objectB = b as JsonObject;
  const keysA = Object.keys(objectA);
  const keysB = Object.keys(objectB);
  if (keysA.length !== keysB.length) return false;
  return keysA.every((key) => Object.hasOwn(objectB, key) && equal(objectA[key], objectB[key]));
};
