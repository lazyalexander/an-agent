import { describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createJsonlMemoryStore } from "../src/memory.ts";
import { ulid } from "../src/ulid.ts";

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

describe("createJsonlMemoryStore", () => {
  test("appends records with increasing seq and never truncates", () => {
    const dir = mkdtempSync(join(tmpdir(), "an-agent-memory-"));
    const path = join(dir, "memory.jsonl");
    const store = createJsonlMemoryStore(path);

    const first = store.append({
      from: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
      from_kind: "human",
      kind: "utterance",
      tags: ["utterance"],
      content: "hi",
      refs: [],
    });
    const second = store.append({
      from: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
      from_kind: "agent",
      kind: "observation",
      tags: ["observation"],
      content: "saw hi",
      refs: [],
    });

    expect(first.seq).toBe(1);
    expect(second.seq).toBe(2);
    expect(first.id).toHaveLength(26);
    expect(store.readAll()).toHaveLength(2);
    expect(readFileSync(path, "utf8").trim().split("\n")).toHaveLength(2);
  });
});
