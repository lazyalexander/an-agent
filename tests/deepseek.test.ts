import { describe, expect, test } from "bun:test";
import { createDeepSeekClient } from "../src/deepseek.ts";
import type { Tool } from "../src/types.ts";

describe("createDeepSeekClient", () => {
  test("posts chat completions and maps a text reply", async () => {
    const calls: unknown[] = [];
    // Bun's fetch type includes `preconnect`; cast keeps injectable fakes assignable.
    const fakeFetch = (async (input, init) => {
      calls.push({ url: String(input), body: JSON.parse(String(init?.body)) });
      return new Response(
        JSON.stringify({
          choices: [{ message: { role: "assistant", content: "ok" } }],
        }),
        { status: 200, headers: { "Content-Type": "application/json" } },
      );
    }) as typeof fetch;

    const complete = createDeepSeekClient({
      apiKey: "test-key",
      model: "deepseek-v4-flash",
      fetch: fakeFetch,
    });

    const reply = await complete({
      messages: [{ role: "user", content: "hi" }],
      tools: [],
    });

    expect(reply).toEqual({ role: "assistant", content: "ok" });
    expect(calls).toEqual([
      {
        url: "https://api.deepseek.com/chat/completions",
        body: {
          model: "deepseek-v4-flash",
          messages: [{ role: "user", content: "hi" }],
          thinking: { type: "disabled" },
        },
      },
    ]);
  });

  test("maps tool calls and sends tool definitions", async () => {
    const fakeFetch = (async (_input, init) => {
      const body = JSON.parse(String(init?.body)) as { tools?: unknown };
      expect(body.tools).toEqual([
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

    const complete = createDeepSeekClient({ apiKey: "k", fetch: fakeFetch });
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
