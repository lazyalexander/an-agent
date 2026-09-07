import { describe, expect, test } from "bun:test";
import { existsSync, mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  loadOrCreateId,
  localAgentId,
  stdinCounterpartId,
  uuidv5,
} from "../src/principals.ts";

describe("uuidv5", () => {
  test("matches the RFC 4122 DNS example", () => {
    expect(uuidv5("6ba7b810-9dad-11d1-80b4-00c04fd430c8", "www.example.com")).toBe(
      "2ed6657d-e927-568b-95e1-2665a8aea6a2",
    );
  });
});

describe("localAgentId", () => {
  test("reuses the persisted agent UUID and does not write human-id", () => {
    const dir = mkdtempSync(join(tmpdir(), "an-agent-ids-"));
    const first = localAgentId(dir);
    const second = localAgentId(dir);
    expect(first).toBe(second);
    expect(loadOrCreateId(join(dir, "agent-id"))).toBe(first);
    expect(readFileSync(join(dir, "agent-id"), "utf8").trim()).toBe(first);
    expect(existsSync(join(dir, "human-id"))).toBe(false);
  });
});

describe("stdinCounterpartId", () => {
  test("is stable per agent, distinct across agents, and not the agent id", () => {
    const a = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    const b = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
    expect(stdinCounterpartId(a)).toBe(stdinCounterpartId(a));
    expect(stdinCounterpartId(a)).not.toBe(stdinCounterpartId(b));
    expect(stdinCounterpartId(a)).not.toBe(a);
    expect(stdinCounterpartId(a)).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-5[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    );
  });
});
