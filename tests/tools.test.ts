import { describe, expect, test } from "bun:test";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { loadTools } from "../src/tools.ts";
import { bash } from "../src/tools/bash.ts";

describe("bash", () => {
  test("runs a command and returns stdout", async () => {
    const out = await bash.execute({ command: "printf 'ok'" });
    expect(out).toBe("ok");
  });

  test("returns stderr and exit code when the command fails", async () => {
    const out = await bash.execute({ command: "printf 'no' >&2; exit 3" });
    expect(out).toContain("exit 3");
    expect(out).toContain("no");
  });
});

describe("loadTools", () => {
  test("enables only named tools from config", () => {
    const dir = mkdtempSync(join(tmpdir(), "an-agent-tools-"));
    const path = join(dir, "tools.json");
    writeFileSync(path, JSON.stringify({ enabled: ["bash"] }));
    const tools = loadTools(path);
    expect(tools.map((t) => t.name)).toEqual(["bash"]);
  });

  test("loads no tools when enabled is empty", () => {
    const dir = mkdtempSync(join(tmpdir(), "an-agent-tools-"));
    const path = join(dir, "tools.json");
    writeFileSync(path, JSON.stringify({ enabled: [] }));
    expect(loadTools(path)).toEqual([]);
  });
});
