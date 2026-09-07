import { describe, expect, test } from "bun:test";
import { createChatCompletionsClient } from "../src/chat-completions.ts";
import type { Tool } from "../src/types.ts";

describe("createChatCompletionsClient", () => {
  test("posts OpenAI-compatible chat completions without vendor fields", async () => {
    const calls: unknown[] = [];
    const fakeFetch = (async (input, init) => {
      calls.push({
        url: String(input),
        auth: new Headers(init?.headers).get("Authorization"),
        body: JSON.parse(String(init?.body)),
      });
      return new Response(
        JSON.stringify({
          choices: [{ message: { role: "assistant", content: "ok" } }],
        }),
        { status: 200 },
      );
    }) as typeof fetch;

    const complete = createChatCompletionsClient({
      apiKey: "test-key",
      model: "any-model",
      baseUrl: "https://example.test/v1",
      fetch: fakeFetch,
    });

    const reply = await complete({
      messages: [{ role: "user", content: "hi" }],
      tools: [],
    });

    expect(reply).toEqual({ role: "assistant", content: "ok" });
    expect(calls).toEqual([
      {
        url: "https://example.test/v1/chat/completions",
        auth: "Bearer test-key",
        body: {
          model: "any-model",
          messages: [{ role: "user", content: "hi" }],
        },
      },
    ]);
  });

  test("merges extraBody for vendor-specific fields", async () => {
    let body: Record<string, unknown> = {};
    const fakeFetch = (async (_input, init) => {
      body = JSON.parse(String(init?.body)) as Record<string, unknown>;
      return new Response(
        JSON.stringify({
          choices: [{ message: { role: "assistant", content: "x" } }],
        }),
        { status: 200 },
      );
    }) as typeof fetch;

    const complete = createChatCompletionsClient({
      apiKey: "k",
      model: "m",
      baseUrl: "https://example.test",
      extraBody: { thinking: { type: "disabled" } },
      fetch: fakeFetch,
    });

    await complete({ messages: [{ role: "user", content: "q" }], tools: [] });

    expect(body.thinking).toEqual({ type: "disabled" });
  });

  test("maps tool calls using the OpenAI function shape", async () => {
    const fakeFetch = (async (_input, init) => {
      const sent = JSON.parse(String(init?.body)) as { tools?: unknown };
      expect(sent.tools).toEqual([
        {
          type: "function",
          function: {
            name: "echo",
            description: "echo",
            parameters: { type: "object" },
          },
        },
      ]);
      return new Response(
        JSON.stringify({
          choices: [
            {
              message: {
                role: "assistant",
                content: null,
                tool_calls: [
                  {
                    id: "c1",
                    function: { name: "echo", arguments: "{\"text\":\"a\"}" },
                  },
                ],
              },
            },
          ],
        }),
        { status: 200 },
      );
    }) as typeof fetch;

    const echo: Tool = {
      name: "echo",
      description: "echo",
      parameters: { type: "object" },
      execute: () => "a",
    };

    const complete = createChatCompletionsClient({
      apiKey: "k",
      model: "m",
      baseUrl: "https://example.test",
      fetch: fakeFetch,
    });

    const reply = await complete({
      messages: [{ role: "user", content: "x" }],
      tools: [echo],
    });

    expect(reply).toEqual({
      role: "assistant",
      content: "",
      tool_calls: [{ id: "c1", name: "echo", arguments: "{\"text\":\"a\"}" }],
    });
  });
});
