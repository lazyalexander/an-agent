import { describe, expect, test } from "bun:test";
import { mkdtempSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import { spawn } from "node:child_process";

const root = join(import.meta.dir, "..");

const runCli = (stdinText: string) =>
  new Promise<{ code: number | null; stderr: string }>((resolve, reject) => {
    const home = mkdtempSync(join(tmpdir(), "an-agent-cli-"));
    const child = spawn("bun", ["src/cli.ts"], {
      cwd: root,
      env: {
        ...process.env,
        HOME: home,
        PATH: `${homedir()}/.bun/bin:${process.env.PATH}`,
      },
      stdio: ["pipe", "pipe", "pipe"],
    });
    let stderr = "";
    child.stderr.on("data", (chunk) => {
      stderr += String(chunk);
    });
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      reject(new Error(`timeout stderr=${stderr}`));
    }, 10000);
    child.on("close", (code) => {
      clearTimeout(timer);
      resolve({ code, stderr });
    });
    child.stdin.write(stdinText);
    child.stdin.end();
  });

describe("cli stdin end", () => {
  test("empty stdin exits 0 without a crash message", async () => {
    const result = await runCli("");
    expect(result.code).toBe(0);
    expect(result.stderr).not.toContain("readline was closed");
  });

  test("/exit exits 0", async () => {
    const result = await runCli("/exit\n");
    expect(result.code).toBe(0);
  });
});
