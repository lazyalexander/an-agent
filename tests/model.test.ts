import { describe, expect, test } from "bun:test";
import { createModelClient } from "../src/model.ts";

describe("createModelClient", () => {
  test("uses the DeepSeek preset by default", async () => {
    const previous = {
      MODEL_PROVIDER: process.env.MODEL_PROVIDER,
      DEEPSEEK_API_KEY: process.env.DEEPSEEK_API_KEY,
    };
    process.env.DEEPSEEK_API_KEY = "ds-key";
    delete process.env.MODEL_PROVIDER;

    try {
      let url = "";
      let body: Record<string, unknown> = {};
      const complete = createModelClient({
        fetch: (async (input, init) => {
          url = String(input);
          body = JSON.parse(String(init?.body)) as Record<string, unknown>;
          return new Response(
            JSON.stringify({ choices: [{ message: { content: "ok" } }] }),
            { status: 200 },
          );
        }) as typeof fetch,
      });

      await complete({ messages: [{ role: "user", content: "hi" }], tools: [] });

      expect(url).toBe("https://api.deepseek.com/chat/completions");
      expect(body.model).toBe("deepseek-v4-flash");
      expect(body.thinking).toEqual({ type: "disabled" });
    } finally {
      if (previous.MODEL_PROVIDER === undefined) delete process.env.MODEL_PROVIDER;
      else process.env.MODEL_PROVIDER = previous.MODEL_PROVIDER;
      if (previous.DEEPSEEK_API_KEY === undefined) delete process.env.DEEPSEEK_API_KEY;
      else process.env.DEEPSEEK_API_KEY = previous.DEEPSEEK_API_KEY;
    }
  });

  test("uses a generic chat completions endpoint when provider is openai-compatible", async () => {
    const previous = {
      MODEL_PROVIDER: process.env.MODEL_PROVIDER,
      MODEL_API_KEY: process.env.MODEL_API_KEY,
      MODEL_BASE_URL: process.env.MODEL_BASE_URL,
      MODEL_NAME: process.env.MODEL_NAME,
    };
    process.env.MODEL_PROVIDER = "openai-compatible";
    process.env.MODEL_API_KEY = "k";
    process.env.MODEL_BASE_URL = "https://api.example.test/v1";
    process.env.MODEL_NAME = "some-model";

    try {
      let url = "";
      let body: Record<string, unknown> = {};
      const complete = createModelClient({
        fetch: (async (input, init) => {
          url = String(input);
          body = JSON.parse(String(init?.body)) as Record<string, unknown>;
          return new Response(
            JSON.stringify({ choices: [{ message: { content: "ok" } }] }),
            { status: 200 },
          );
        }) as typeof fetch,
      });

      await complete({ messages: [{ role: "user", content: "hi" }], tools: [] });

      expect(url).toBe("https://api.example.test/v1/chat/completions");
      expect(body.model).toBe("some-model");
      expect(body.thinking).toBeUndefined();
    } finally {
      for (const [key, value] of Object.entries(previous)) {
        if (value === undefined) delete process.env[key];
        else process.env[key] = value;
      }
    }
  });
});
