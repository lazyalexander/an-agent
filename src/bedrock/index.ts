/**
 * Safe JSON data for the rest of the program, which is ordinary TypeScript.
 * Operations copy; results are frozen. No Time/Random/Binary — those are not data.
 */
export type { Dict, JsonArray, JsonObject, JsonPrimitive, JsonValue } from "./types.ts";
export { clone, equal, hold, isJsonValue } from "./json.ts";
export { dict } from "./dict.ts";
export { list } from "./list.ts";
