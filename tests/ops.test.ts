import { describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createFileOps, createOtlpOps, resolveOps, timed } from "../src/ops.ts";

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

describe("resolveOps", () => {
  test("is a noop when debug and otlp are off", () => {
    const ops = resolveOps({ env: {} });
    expect(() => ops.record({ kind: "latency", level: "info", name: "x", ms: 1 })).not.toThrow();
  });

  test("writes a debug file when debugPath is set", () => {
    const dir = mkdtempSync(join(tmpdir(), "an-agent-ops-"));
    const path = join(dir, "debug.jsonl");
    const ops = resolveOps({ debugPath: path, env: {} });
    ops.record({ kind: "error", level: "error", name: "cli", error: "x" });
    const line = JSON.parse(readFileSync(path, "utf8").trim());
    expect(line).toMatchObject({ name: "cli", error: "x" });
  });
});

describe("createOtlpOps", () => {
  test("posts a log record to the OTLP logs endpoint", async () => {
    const posts: { url: string; body: unknown }[] = [];
    const fakeFetch = (async (input, init) => {
      posts.push({ url: String(input), body: JSON.parse(String(init?.body)) });
      return new Response("{}", { status: 200 });
    }) as typeof fetch;

    const ops = createOtlpOps({
      endpoint: "https://otel.example/v1/logs",
      fetch: fakeFetch,
    });
    ops.record({ kind: "latency", level: "info", name: "model.complete", ms: 12 });
    await new Promise((r) => setTimeout(r, 20));

    expect(posts).toHaveLength(1);
    expect(posts[0]?.url).toBe("https://otel.example/v1/logs");
    const body = posts[0]?.body as {
      resourceLogs: { scopeLogs: { logRecords: { body: { stringValue: string } }[] }[] }[];
    };
    expect(body.resourceLogs[0]?.scopeLogs[0]?.logRecords[0]?.body.stringValue).toBe("model.complete");
  });
});

