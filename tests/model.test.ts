import { describe, expect, test } from "bun:test";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createModelClient, loadModelSettings } from "../src/model.ts";

const fakeFetchRecording = (into: { url: string; body: Record<string, unknown> }) =>
  (async (input, init) => {
    into.url = String(input);
    into.body = JSON.parse(String(init?.body)) as Record<string, unknown>;
    return new Response(
      JSON.stringify({ choices: [{ message: { content: "ok" } }] }),
      { status: 200 },
    );
  }) as typeof fetch;

describe("loadModelSettings", () => {
  test("reads file config and lets env override", () => {
    const dir = mkdtempSync(join(tmpdir(), "an-agent-model-"));
    const path = join(dir, "model.json");
    writeFileSync(
      path,
      JSON.stringify({
        baseUrl: "https://file.example/v1",
        model: "file-model",
        extraBody: { thinking: { type: "disabled" } },
      }),
    );

    const fromFile = loadModelSettings({}, path);
    expect(fromFile).toEqual({
      baseUrl: "https://file.example/v1",
      model: "file-model",
      extraBody: { thinking: { type: "disabled" } },
      apiKeyEnv: "MODEL_API_KEY",
    });

    const fromEnv = loadModelSettings(
      {
        MODEL_BASE_URL: "https://env.example/v1",
        MODEL_NAME: "env-model",
        MODEL_API_KEY: "k",
        MODEL_EXTRA_BODY: "{}",
      },
      path,
    );
    expect(fromEnv.baseUrl).toBe("https://env.example/v1");
    expect(fromEnv.model).toBe("env-model");
    expect(fromEnv.apiKey).toBe("k");
    expect(fromEnv.extraBody).toEqual({});
  });
});

describe("createModelClient", () => {
  test("posts to settings.baseUrl with extraBody from settings", async () => {
    const seen = { url: "", body: {} as Record<string, unknown> };
    const complete = createModelClient({
      settings: {
        baseUrl: "https://api.example.test/v1",
        model: "some-model",
        apiKey: "k",
        extraBody: { thinking: { type: "disabled" } },
      },
      fetch: fakeFetchRecording(seen),
    });

    await complete({ messages: [{ role: "user", content: "hi" }], tools: [] });

    expect(seen.url).toBe("https://api.example.test/v1/chat/completions");
    expect(seen.body.model).toBe("some-model");
    expect(seen.body.thinking).toEqual({ type: "disabled" });
  });
});
