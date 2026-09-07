import { describe, expect, test } from "bun:test";
import { ulid } from "../../src/memory/index.ts";

describe("ulid", () => {
  test("returns 26 crockford characters and two calls differ", () => {
    const a = ulid();
    const b = ulid();
    expect(a).toHaveLength(26);
    expect(b).toHaveLength(26);
    expect(a).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
    expect(a).not.toBe(b);
  });
});
