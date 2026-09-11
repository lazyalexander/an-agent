import { describe, expect, test } from "bun:test";
import {
  camelCase,
  contain,
  deduplicate,
  difference,
  hyphenate,
  intersection,
  isNullable,
  makeArray,
  omit,
  pick,
  snakeCase,
  Time,
  union,
  without,
} from "../../src/bedrock/opkit/index.ts";

describe("array", () => {
  test("set helpers copy and do not splice the input", () => {
    const left = [1, 2, 3];
    expect(contain(left, [2, 1])).toBe(true);
    expect(intersection(left, [3, 4])).toEqual([3]);
    expect(difference(left, [1])).toEqual([2, 3]);
    expect(union(left, [3, 4])).toEqual([1, 2, 3, 4]);
    expect(deduplicate([1, 1, 2])).toEqual([1, 2]);
    expect(without(left, 2)).toEqual([1, 3]);
    expect(left).toEqual([1, 2, 3]);
    expect(makeArray(1)).toEqual([1]);
    expect(makeArray(null)).toEqual([]);
  });
});

describe("types", () => {
  test("pick and omit return shallow copies", () => {
    const source = { a: 1, b: 2, c: 3 };
    expect(pick(source, ["a", "c"])).toEqual({ a: 1, c: 3 });
    expect(omit(source, ["b"])).toEqual({ a: 1, c: 3 });
    expect(source).toEqual({ a: 1, b: 2, c: 3 });
    expect(isNullable(undefined)).toBe(true);
  });
});

describe("string", () => {
  test("case conversions", () => {
    expect(camelCase("foo-bar_baz")).toBe("fooBarBaz");
    expect(hyphenate("fooBar")).toBe("foo-bar");
    expect(snakeCase("fooBar")).toBe("foo_bar");
  });
});

describe("Time", () => {
  test("parses durations and formats UTC ISO from a given millisecond stamp", () => {
    expect(Time.parse("1h2m")).toBe(Time.hour + 2 * Time.minute);
    expect(Time.format(Time.hour)).toBe("1h");
    expect(Time.iso(0)).toBe("1970-01-01T00:00:00.000Z");
  });
});
