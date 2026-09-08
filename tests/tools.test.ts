import { describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { loadTools } from "../src/tools.ts";
import { bash, createBash } from "../src/tools/bash.ts";

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

describe("bash guardrails", () => {
  test("kills a command that exceeds the timeout", async () => {
    const tool = createBash({ timeoutMs: 200, killGraceMs: 100 });
    const started = Date.now();
    const out = await tool.execute({ command: "sleep 30" });
    expect(Date.now() - started).toBeLessThan(5000);
    expect(out).toContain("timed out");
    expect(out).toContain("killed");
  });

  test("escalates to SIGKILL when SIGTERM is ignored", async () => {
    const tool = createBash({ timeoutMs: 200, killGraceMs: 100 });
    const started = Date.now();
    const out = await tool.execute({ command: "trap '' TERM; sleep 30" });
    expect(Date.now() - started).toBeLessThan(5000);
    expect(out).toContain("timed out");
  });

  test("truncates output beyond the cap", async () => {
    const tool = createBash({ maxOutputBytes: 16 });
    const out = await tool.execute({ command: "seq 1 100" });
    expect(out).toContain("[stdout truncated]");
    expect(out.length).toBeLessThan(100);
  });

  test("aborting the context signal cancels the command", async () => {
    const tool = createBash({ timeoutMs: 30_000 });
    const ctrl = new AbortController();
    setTimeout(() => ctrl.abort(), 100);
    const started = Date.now();
    const out = await tool.execute({ command: "sleep 30" }, { signal: ctrl.signal });
    expect(Date.now() - started).toBeLessThan(5000);
    expect(out).toContain("cancelled");
  });

  test("does not spawn when the signal is already aborted", async () => {
    const tool = createBash({ timeoutMs: 30_000 });
    const ctrl = new AbortController();
    ctrl.abort();
    const started = Date.now();
    const out = await tool.execute({ command: "sleep 30" }, { signal: ctrl.signal });
    expect(Date.now() - started).toBeLessThan(1000);
    expect(out).toContain("cancelled");
  });

  test("kills background children in the process group on timeout", async () => {
    const dir = mkdtempSync(join(tmpdir(), "an-agent-bash-"));
    const pidFile = join(dir, "sleep.pid");
    const tool = createBash({ timeoutMs: 400, killGraceMs: 200 });
    const out = await tool.execute({
      command: `sleep 30 & echo $! > "${pidFile}"; wait`,
    });
    expect(out).toContain("timed out");
    const pid = Number(readFileSync(pidFile, "utf8").trim());
    expect(pid).toBeGreaterThan(0);
    await Bun.sleep(150);
    let alive = true;
    try {
      process.kill(pid, 0);
    } catch {
      alive = false;
    }
    expect(alive).toBe(false);
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
