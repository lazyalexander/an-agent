/** JSON-safe primitives. No undefined, bigint, or NaN. */
export type JsonPrimitive = string | number | boolean | null;

export type JsonObject = { readonly [key: string]: JsonValue };

export type JsonArray = readonly JsonValue[];

export type JsonValue = JsonPrimitive | JsonObject | JsonArray;

/** Readonly string map. Values stay JSON-safe. */
export type Dict<T extends JsonValue = JsonValue> = { readonly [key: string]: T };
