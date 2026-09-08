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

describe("retries and timeouts", () => {
  test("retries a 429 honoring Retry-After and then succeeds", async () => {
    let n = 0;
    const fakeFetch = (async (_input, _init) => {
      n += 1;
      if (n === 1) {
        return new Response("rate limited", {
          status: 429,
          headers: { "Retry-After": "0" },
        });
      }
      return new Response(
        JSON.stringify({ choices: [{ message: { content: "ok" } }] }),
        { status: 200 },
      );
    }) as typeof fetch;

    const complete = createChatCompletionsClient({
      apiKey: "k",
      model: "m",
      baseUrl: "https://example.test",
      fetch: fakeFetch,
      retryBaseMs: 1,
    });

    const reply = await complete({ messages: [], tools: [] });

    expect(reply.content).toBe("ok");
    expect(n).toBe(2);
  });

  test("retries a network failure and then succeeds", async () => {
    let n = 0;
    const fakeFetch = (async (_input, _init) => {
      n += 1;
      if (n === 1) throw new TypeError("fetch failed");
      return new Response(
        JSON.stringify({ choices: [{ message: { content: "ok" } }] }),
        { status: 200 },
      );
    }) as typeof fetch;

    const complete = createChatCompletionsClient({
      apiKey: "k",
      model: "m",
      baseUrl: "https://example.test",
      fetch: fakeFetch,
      retryBaseMs: 1,
    });

    const reply = await complete({ messages: [], tools: [] });

    expect(reply.content).toBe("ok");
    expect(n).toBe(2);
  });

  test("does not retry a 400", async () => {
    let n = 0;
    const fakeFetch = (async (_input, _init) => {
      n += 1;
      return new Response("bad request", { status: 400 });
    }) as typeof fetch;

    const complete = createChatCompletionsClient({
      apiKey: "k",
      model: "m",
      baseUrl: "https://example.test",
      fetch: fakeFetch,
      retryBaseMs: 1,
    });

    await expect(complete({ messages: [], tools: [] })).rejects.toThrow("HTTP 400");
    expect(n).toBe(1);
  });

  test("gives up after maxAttempts on persistent 503", async () => {
    let n = 0;
    const fakeFetch = (async (_input, _init) => {
      n += 1;
      return new Response("unavailable", { status: 503 });
    }) as typeof fetch;

    const complete = createChatCompletionsClient({
      apiKey: "k",
      model: "m",
      baseUrl: "https://example.test",
      fetch: fakeFetch,
      maxAttempts: 3,
      retryBaseMs: 1,
    });

    await expect(complete({ messages: [], tools: [] })).rejects.toThrow(
      "failed after 3 attempts",
    );
    expect(n).toBe(3);
  });

  test("times out a wedged request and reports it after retries", async () => {
    let n = 0;
    const fakeFetch = ((_input, init) => {
      n += 1;
      return new Promise((_resolve, reject) => {
        init?.signal?.addEventListener("abort", () =>
          reject(new DOMException("The operation was aborted.", "AbortError")),
        );
      });
    }) as typeof fetch;

    const complete = createChatCompletionsClient({
      apiKey: "k",
      model: "m",
      baseUrl: "https://example.test",
      fetch: fakeFetch,
      timeoutMs: 50,
      maxAttempts: 2,
      retryBaseMs: 1,
    });

    await expect(complete({ messages: [], tools: [] })).rejects.toThrow(
      "failed after 2 attempts: chat completions timed out",
    );
    expect(n).toBe(2);
  });

  test("an external abort cancels immediately without retrying", async () => {
    const ctrl = new AbortController();
    let n = 0;
    const fakeFetch = ((_input, init) => {
      n += 1;
      return new Promise((_resolve, reject) => {
        init?.signal?.addEventListener("abort", () =>
          reject(new DOMException("The operation was aborted.", "AbortError")),
        );
      });
    }) as typeof fetch;

    const complete = createChatCompletionsClient({
      apiKey: "k",
      model: "m",
      baseUrl: "https://example.test",
      fetch: fakeFetch,
      retryBaseMs: 60_000,
    });

    const promise = complete({ messages: [], tools: [], signal: ctrl.signal });
    setTimeout(() => ctrl.abort(), 20);

    await expect(promise).rejects.toThrow("turn cancelled");
    expect(n).toBe(1);
  });
});

describe("response validation and repair", () => {
  test("retries a response with no choices, then fails clearly", async () => {
    let n = 0;
    const fakeFetch = (async (_input, _init) => {
      n += 1;
      return new Response(JSON.stringify({}), { status: 200 });
    }) as typeof fetch;

    const complete = createChatCompletionsClient({
      apiKey: "k",
      model: "m",
      baseUrl: "https://example.test",
      fetch: fakeFetch,
      maxAttempts: 2,
      retryBaseMs: 1,
    });

    await expect(complete({ messages: [], tools: [] })).rejects.toThrow("no choices");
    expect(n).toBe(2);
  });

  test("fails fast when the choice has no message", async () => {
    let n = 0;
    const fakeFetch = (async (_input, _init) => {
      n += 1;
      return new Response(JSON.stringify({ choices: [{}] }), { status: 200 });
    }) as typeof fetch;

    const complete = createChatCompletionsClient({
      apiKey: "k",
      model: "m",
      baseUrl: "https://example.test",
      fetch: fakeFetch,
      retryBaseMs: 1,
    });

    await expect(complete({ messages: [], tools: [] })).rejects.toThrow("no message");
    expect(n).toBe(1);
  });

  test("repairs a tool_call missing id and with object arguments", async () => {
    const fakeFetch = (async (_input, _init) =>
      new Response(
        JSON.stringify({
          choices: [
            {
              message: {
                content: null,
                tool_calls: [{ function: { name: "echo", arguments: { text: "a" } } }],
              },
            },
          ],
        }),
        { status: 200 },
      )) as typeof fetch;

    const complete = createChatCompletionsClient({
      apiKey: "k",
      model: "m",
      baseUrl: "https://example.test",
      fetch: fakeFetch,
    });

    const reply = await complete({ messages: [], tools: [] });

    expect(reply.tool_calls).toHaveLength(1);
    const call = reply.tool_calls![0]!;
    expect(call.id).not.toBe("");
    expect(call.name).toBe("echo");
    expect(call.arguments).toBe("{\"text\":\"a\"}");
    expect(() => JSON.parse(call.arguments)).not.toThrow();
  });
});
