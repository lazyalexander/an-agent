import { describe, expect, test } from "bun:test";
import type { MemoryEvent } from "../../src/bedrock/memkit/types.ts";
import {
  FORGET_TAG,
  OUTSIDE_TAG,
  fileConflicts,
  fileCoversPath,
  isMaskedOutside,
  permitImplied,
  toolTag,
} from "../../src/tooltag/index.ts";

const event = (over: Partial<MemoryEvent>): MemoryEvent => ({
  v: 1,
  id: "id",
  seq: 1,
  ts: "2026-01-01T00:00:00.000Z",
  from: "agent-1",
  from_kind: "agent",
  kind: "observation",
  content: "",
  tags: [],
  refs: [],
  ...over,
});

describe("file face", () => {
  test("r vs r does not conflict; r vs w on the same path does", () => {
    const read = toolTag.read("/src/a.ts").file;
    const write = toolTag.write("/src/a.ts").file;
    expect(fileConflicts(read, toolTag.read("/src/a.ts").file)).toBe(false);
    expect(fileConflicts(read, write)).toBe(true);
  });

  test("recursive dir covers descendants; a file write does not cover the parent", () => {
    const tree = toolTag.read("/src", { recursive: true }).file;
    expect(fileCoversPath(tree, "/src/lib/a.ts")).toBe(true);
    expect(fileCoversPath(toolTag.write("/src/lib/a.ts").file, "/src")).toBe(false);
  });

  test("unbounded conflicts with any named path; none conflicts with nothing", () => {
    expect(fileConflicts(toolTag.unbounded().file, toolTag.read("/x").file)).toBe(true);
    expect(fileConflicts(toolTag.none().file, toolTag.write("/x").file)).toBe(false);
  });
});

describe("permit face", () => {
  test("forbidden closes downward; go does not transfer to a child", () => {
    expect(
      permitImplied({ path: "/src", recursive: true, permit: "forbidden" }, "/src/a.ts"),
    ).toBe("forbidden");
    expect(permitImplied({ path: "/src", recursive: true, permit: "go" }, "/src/a.ts")).toBe(
      undefined,
    );
    expect(permitImplied({ path: "/src", permit: "go" }, "/src")).toBe("go");
    expect(permitImplied({ path: "/src", permit: "ask" }, "/src/a.ts")).toBe(undefined);
    expect(permitImplied({ path: "/src", recursive: true, permit: "ask" }, "/src/a.ts")).toBe(
      "ask",
    );
  });
});

describe("memory face", () => {
  test("forget masks the remember id and derived outside events", () => {
    const remember = event({
      id: "rem-1",
      seq: 1,
      kind: "utterance",
      tags: [OUTSIDE_TAG],
      content: "page",
    });
    const quote = event({
      id: "rem-2",
      seq: 2,
      tags: [OUTSIDE_TAG],
      refs: ["rem-1"],
      content: "quote",
    });
    const forget = event({
      id: "fg-1",
      seq: 80,
      tags: [FORGET_TAG],
      refs: ["rem-1"],
      content: "forget rem-1",
    });
    const later = [remember, quote, forget];
    expect(isMaskedOutside(remember, [remember, quote])).toBe(false);
    expect(isMaskedOutside(remember, later)).toBe(true);
    expect(isMaskedOutside(quote, later)).toBe(true);
  });

  test("forget does not mask a different remember id", () => {
    const a = event({ id: "rem-a", tags: [OUTSIDE_TAG] });
    const b = event({ id: "rem-b", seq: 2, tags: [OUTSIDE_TAG] });
    const forget = event({ id: "fg", seq: 3, tags: [FORGET_TAG], refs: ["rem-a"] });
    const events = [a, b, forget];
    expect(isMaskedOutside(a, events)).toBe(true);
    expect(isMaskedOutside(b, events)).toBe(false);
  });
});

describe("toolTag constructors keep faces independent", () => {
  test("unbounded ask ignore; remember is none+go; write default ask", () => {
    expect(toolTag.unbounded()).toEqual({
      file: { op: "unbounded" },
      permit: "ask",
      memory: { op: "ignore" },
    });
    expect(toolTag.remember("title")).toEqual({
      file: { op: "none" },
      permit: "go",
      memory: { op: "remember", aspect: "title" },
    });
    expect(toolTag.write("/x").permit).toBe("ask");
    expect(toolTag.read("/x").permit).toBe("go");
    expect(toolTag.forget("rem-1").memory).toEqual({ op: "forget", rememberId: "rem-1" });
  });
});
