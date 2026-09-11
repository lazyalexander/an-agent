import { describe, expect, test } from "bun:test";
import { clone, dict, equal, hold, isJsonValue, list } from "../../src/bedrock/index.ts";

describe("isJsonValue", () => {
  test("accepts plain JSON and rejects the usual footguns", () => {
    expect(isJsonValue({ a: [1, "x", true, null] })).toBe(true);
    expect(isJsonValue(undefined)).toBe(false);
    expect(isJsonValue(() => 1)).toBe(false);
    expect(isJsonValue(Number.NaN)).toBe(false);
    expect(isJsonValue(new Date())).toBe(false);
  });
});

describe("hold", () => {
  test("returns a frozen copy and leaves the input mutable", () => {
    const input = { n: 1, inner: { k: "v" } };
    const held = hold(input);
    expect(held).toEqual(input);
    expect(Object.isFrozen(held)).toBe(true);
    expect(Object.isFrozen(held.inner)).toBe(true);
    input.n = 2;
    expect(held.n).toBe(1);
    expect(() => {
      (held as { n: number }).n = 3;
    }).toThrow();
  });

  test("throws on non-JSON values", () => {
    expect(() => hold({ fn: () => 1 } as never)).toThrow(/JSON-safe/);
  });
});

describe("clone", () => {
  test("is deep and does not freeze the copy", () => {
    const input = { inner: { k: 1 } };
    const copied = clone(input);
    expect(copied).toEqual(input);
    expect(copied).not.toBe(input);
    expect(copied.inner).not.toBe(input.inner);
    expect(Object.isFrozen(copied)).toBe(false);
    copied.inner.k = 2;
    expect(input.inner.k).toBe(1);
  });
});

describe("equal", () => {
  test("compares objects without regard to key order", () => {
    expect(equal({ a: 1, b: 2 }, { b: 2, a: 1 })).toBe(true);
    expect(equal([1, { a: 1 }], [1, { a: 1 }])).toBe(true);
    expect(equal([1, 2], [2, 1])).toBe(false);
  });
});

describe("dict", () => {
  test("set and remove do not mutate the original", () => {
    const base = dict.of({ a: 1 });
    const next = dict.set(base, "b", 2);
    const gone = dict.remove(next, "a");
    expect(base).toEqual({ a: 1 });
    expect(next).toEqual({ a: 1, b: 2 });
    expect(gone).toEqual({ b: 2 });
    expect(dict.get(next, "b")).toBe(2);
    expect(dict.has(gone, "a")).toBe(false);
    expect(() => {
      (next as { a: number }).a = 9;
    }).toThrow();
  });
});

describe("list", () => {
  test("append and concat copy", () => {
    const xs = list.of(1, 2);
    const ys = list.append(xs, 3);
    const zs = list.concat(ys, list.of(4));
    expect(xs).toEqual([1, 2]);
    expect(ys).toEqual([1, 2, 3]);
    expect(zs).toEqual([1, 2, 3, 4]);
    expect(list.at(zs, 0)).toBe(1);
    expect(list.slice(zs, 1, 3)).toEqual([2, 3]);
    expect(() => {
      (xs as number[]).push(9);
    }).toThrow();
  });
});
