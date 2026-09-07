import { describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createFileOps, timed } from "../src/ops.ts";

describe("createFileOps", () => {
  test("records latency and errors as append-only jsonl", async () => {
    const dir = mkdtempSync(join(tmpdir(), "an-agent-ops-"));
    const path = join(dir, "ops.jsonl");
    const ops = createFileOps(path);

    const value = await timed(ops, "model.complete", async () => 7);
    expect(value).toBe(7);

    await expect(
      timed(ops, "model.complete", async () => {
        throw new Error("boom");
      }),
    ).rejects.toThrow("boom");

    const lines = readFileSync(path, "utf8").trim().split("\n").map((line) => JSON.parse(line));
    expect(lines).toHaveLength(2);
    expect(lines[0]).toMatchObject({ kind: "latency", name: "model.complete", level: "info" });
    expect(typeof lines[0].ms).toBe("number");
    expect(lines[1]).toMatchObject({
      kind: "error",
      name: "model.complete",
      level: "error",
      error: "boom",
    });
    expect(typeof lines[1].ms).toBe("number");
  });
});
