import { describe, expect, test } from "bun:test";
import { parseCliArgs } from "../src/args.ts";

describe("parseCliArgs", () => {
  test("reads --debug-ops path", () => {
    expect(parseCliArgs(["--debug-ops", "/tmp/ops.jsonl"])).toEqual({
      debugOps: "/tmp/ops.jsonl",
    });
  });

  test("defaults to no debug file", () => {
    expect(parseCliArgs([])).toEqual({});
  });
});
