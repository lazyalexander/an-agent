import { describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { loadOrCreateId, localPrincipals } from "../src/principals.ts";

describe("localPrincipals", () => {
  test("reuses persisted agent and human UUIDs", () => {
    const dir = mkdtempSync(join(tmpdir(), "an-agent-ids-"));
    const first = localPrincipals(dir);
    const second = localPrincipals(dir);
    expect(first.agentId).toBe(second.agentId);
    expect(first.humanId).toBe(second.humanId);
    expect(first.agentId).not.toBe(first.humanId);
    expect(loadOrCreateId(join(dir, "agent-id"))).toBe(first.agentId);
    expect(readFileSync(join(dir, "human-id"), "utf8").trim()).toBe(first.humanId);
  });
});
