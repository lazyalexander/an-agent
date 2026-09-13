import { describe, expect, test } from "bun:test";
import { defineTool, resolveTag, tagOf, toolTag } from "../../src/tool/index.ts";
import type { Tool } from "../../src/tool/core.ts";

describe("defineTool", () => {
  test("keeps the core tool fields and copies tag onto the instance", () => {
    const tool = defineTool({
      name: "echo",
      description: "echo",
      parameters: {},
      tag: toolTag.none(),
      execute: (args) => String(args.text ?? ""),
    });
    expect(tool.name).toBe("echo");
    expect(tool.tag).toEqual(toolTag.none());
    expect(tagOf(tool).file.op).toBe("none");
  });
});

describe("tagOf", () => {
  test("treats an untagged tool as unbounded ask ignore", () => {
    const tool: Tool = {
      name: "raw",
      description: "raw",
      parameters: {},
      execute: () => "ok",
    };
    expect(tagOf(tool)).toEqual(toolTag.unbounded());
  });
});

describe("resolveTag", () => {
  test("fills path without widening file op", () => {
    const next = resolveTag(toolTag.read("/src"), { path: "/src/a.ts" });
    expect(next.file).toEqual({ op: "r", path: "/src/a.ts", recursive: undefined });
  });

  test("narrows unbounded when a path is given", () => {
    const next = resolveTag(toolTag.unbounded(), { fileOp: "w", path: "/tmp/x" });
    expect(next.file).toEqual({ op: "w", path: "/tmp/x", recursive: undefined });
  });

  test("rejects widening file or permit", () => {
    expect(() => resolveTag(toolTag.read("/x"), { fileOp: "unbounded" })).toThrow(/widen/);
    expect(() => resolveTag(toolTag.unbounded("ask"), { permit: "go" })).toThrow(/widen/);
  });

  test("ignore may become remember; cannot become forget", () => {
    const remembered = resolveTag(toolTag.none(), { memoryOp: "remember", aspect: "title" });
    expect(remembered.memory).toEqual({ op: "remember", aspect: "title" });
    expect(() => resolveTag(toolTag.none(), { memoryOp: "forget" })).toThrow(/forget/);
  });
});
