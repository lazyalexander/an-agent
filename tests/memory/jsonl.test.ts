import { describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createJsonlMemoryStore } from "../../src/memory/index.ts";

const tempPath = (): string =>
  join(mkdtempSync(join(tmpdir(), "an-agent-memory-")), "memory.jsonl");

const baseInput = {
  from: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
  from_kind: "unknown" as const,
  kind: "utterance" as const,
  session: "11111111-1111-1111-1111-111111111111",
  tags: [] as const,
  refs: [] as const,
};

describe("createJsonlMemoryStore", () => {
  test("appends records with increasing seq and never truncates", () => {
    const path = tempPath();
    const store = createJsonlMemoryStore(path);

    const first = store.append({ ...baseInput, content: "hi" });
    const second = store.append({
      ...baseInput,
      from: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
      from_kind: "agent",
      kind: "observation",
      content: "saw hi",
    });

    expect(first.seq).toBe(1);
    expect(second.seq).toBe(2);
    expect(first.session).toBe(baseInput.session);
    expect(first.from_kind).toBe("unknown");
    expect(first.tags).toEqual([]);
    expect(first.id).toHaveLength(26);
    expect(store.readAll()).toHaveLength(2);
    expect(readFileSync(path, "utf8").trim().split("\n")).toHaveLength(2);
  });

  test("continues seq from the last record without recounting lines", () => {
    const path = tempPath();
    writeFileSync(
      path,
      `${JSON.stringify({
        v: 1,
        id: "01TEST00000000000000000001",
        seq: 7,
        ts: "2026-01-01T00:00:00.000Z",
        from: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        from_kind: "agent",
        kind: "utterance",
        session: "11111111-1111-1111-1111-111111111111",
        content: "prior",
        tags: [],
        refs: [],
      })}\n`,
    );
    const store = createJsonlMemoryStore(path);
    const next = store.append({ ...baseInput, content: "after" });
    expect(next.seq).toBe(8);
    expect(store.append({ ...baseInput, content: "again" }).seq).toBe(9);
  });

  test("reads old records that have no session and does not rewrite them", () => {
    const path = tempPath();
    const oldLine = JSON.stringify({
      v: 1,
      id: "01TEST00000000000000000002",
      seq: 1,
      ts: "2026-01-01T00:00:00.000Z",
      from: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
      from_kind: "human",
      kind: "utterance",
      content: "legacy",
      tags: ["utterance"],
      refs: [],
    });
    writeFileSync(path, `${oldLine}\n`);
    const store = createJsonlMemoryStore(path);
    const [legacy] = store.readAll();
    expect(legacy?.content).toBe("legacy");
    expect(legacy?.session).toBeUndefined();
    expect(readFileSync(path, "utf8").trim()).toBe(oldLine);
  });

  test("throws on a corrupt non-empty line", () => {
    const path = tempPath();
    writeFileSync(path, "not-json\n");
    const store = createJsonlMemoryStore(path);
    expect(() => store.readAll()).toThrow();
  });
});
