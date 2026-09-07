import { describe, expect, test } from "bun:test";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { applyEnvFile } from "../src/load-env.ts";

describe("applyEnvFile", () => {
  test("sets missing keys and does not override existing env", () => {
    const dir = mkdtempSync(join(tmpdir(), "an-agent-env-"));
    const path = join(dir, ".env");
    writeFileSync(path, "NEW_ONLY=from-file\nALREADY=from-file\n");
    const env: Record<string, string | undefined> = { ALREADY: "from-process" };

    applyEnvFile(path, env);

    expect(env.NEW_ONLY).toBe("from-file");
    expect(env.ALREADY).toBe("from-process");
  });

  test("skips comments and blank lines and strips quotes", () => {
    const dir = mkdtempSync(join(tmpdir(), "an-agent-env-"));
    const path = join(dir, ".env");
    writeFileSync(path, "# comment\n\nFOO=\"bar baz\"\n");
    const env: Record<string, string | undefined> = {};

    applyEnvFile(path, env);

    expect(env.FOO).toBe("bar baz");
  });
});
