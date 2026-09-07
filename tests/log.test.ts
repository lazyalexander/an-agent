import { describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { appendLog, createFileLog } from "../src/log.ts";

describe("appendLog", () => {
  test("appends one JSON object per line and never truncates", () => {
    const dir = mkdtempSync(join(tmpdir(), "an-agent-log-"));
    const path = join(dir, "log.jsonl");
    const log = createFileLog(path);

    appendLog(log, { type: "user", content: "hi" });
    appendLog(log, { type: "assistant", content: "hello" });

    const lines = readFileSync(path, "utf8").trim().split("\n");
    expect(lines).toHaveLength(2);
    expect(JSON.parse(lines[0]!).type).toBe("user");
    expect(JSON.parse(lines[1]!).type).toBe("assistant");
    expect(JSON.parse(lines[0]!).ts).toEqual(expect.any(String));
  });
});
