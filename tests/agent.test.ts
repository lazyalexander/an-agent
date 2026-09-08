import { describe, expect, test } from "bun:test";
import {
  runUntilIdle,
  step,
  type AgentState,
  type ModelClient,
  type Tool,
} from "../src/agent.ts";

const fakeComplete: ModelClient = async () => ({
  role: "assistant",
  content: "hello",
});

describe("step", () => {
  test("appends the assistant message and does not mutate input state", async () => {
    const state: AgentState = {
      messages: [{ role: "user", content: "hi" }],
    };

    const next = await step(state, { complete: fakeComplete, tools: [] });

    expect(state.messages).toHaveLength(1);
    expect(next.messages).toEqual([
      { role: "user", content: "hi" },
      { role: "assistant", content: "hello" },
    ]);
  });
});

describe("step with tools", () => {
  test("executes tool calls and appends tool messages", async () => {
    const complete: ModelClient = async () => ({
      role: "assistant",
      content: "",
      tool_calls: [
        { id: "c1", name: "echo", arguments: JSON.stringify({ text: "ping" }) },
      ],
    });

    const echo: Tool = {
      name: "echo",
      description: "echo text",
      parameters: {
        type: "object",
        properties: { text: { type: "string" } },
        required: ["text"],
      },
      execute: (args) => String(args.text),
    };

    const next = await step(
      { messages: [{ role: "user", content: "echo ping" }] },
      { complete, tools: [echo] },
    );

    expect(next.messages).toEqual([
      { role: "user", content: "echo ping" },
      {
        role: "assistant",
        content: "",
        tool_calls: [
          { id: "c1", name: "echo", arguments: JSON.stringify({ text: "ping" }) },
        ],
      },
      { role: "tool", tool_call_id: "c1", content: "ping" },
    ]);
  });

  test("missing tool yields an error tool message", async () => {
    const complete: ModelClient = async () => ({
      role: "assistant",
      content: "",
      tool_calls: [{ id: "c1", name: "nope", arguments: "{}" }],
    });

    const next = await step(
      { messages: [{ role: "user", content: "x" }] },
      { complete, tools: [] },
    );

    const last = next.messages.at(-1);
    expect(last).toMatchObject({ role: "tool", tool_call_id: "c1" });
    expect(String((last as { content: string }).content)).toContain("nope");
  });
});

describe("runUntilIdle", () => {
  test("calls the model again after tools until a plain assistant message", async () => {
    let n = 0;
    const complete: ModelClient = async () => {
      n += 1;
      if (n === 1) {
        return {
          role: "assistant",
          content: "",
          tool_calls: [{ id: "c1", name: "echo", arguments: JSON.stringify({ text: "pong" }) }],
        };
      }
      return { role: "assistant", content: "done pong" };
    };

    const echo: Tool = {
      name: "echo",
      description: "echo",
      parameters: { type: "object", properties: { text: { type: "string" } } },
      execute: (args) => String(args.text),
    };

    const next = await runUntilIdle(
      { messages: [{ role: "user", content: "go" }] },
      { complete, tools: [echo] },
    );

    expect(n).toBe(2);
    expect(next.messages.at(-1)).toEqual({ role: "assistant", content: "done pong" });
  });

  test("throws when maxSteps is exceeded", async () => {
    const complete: ModelClient = async () => ({
      role: "assistant",
      content: "",
      tool_calls: [{ id: "c1", name: "echo", arguments: JSON.stringify({ text: "x" }) }],
    });
    const echo: Tool = {
      name: "echo",
      description: "echo",
      parameters: {},
      execute: () => "x",
    };

    await expect(
      runUntilIdle(
        { messages: [{ role: "user", content: "loop" }] },
        { complete, tools: [echo] },
        2,
      ),
    ).rejects.toThrow("maxSteps");
  });
});

describe("runUntilIdle cancellation", () => {
  test("passes the deps signal to tool execution", async () => {
    let seen: AbortSignal | undefined;
    const complete: ModelClient = async () => ({
      role: "assistant",
      content: "",
      tool_calls: [{ id: "c1", name: "probe", arguments: "{}" }],
    });
    const probe: Tool = {
      name: "probe",
      description: "probe",
      parameters: {},
      execute: (_args, ctx) => {
        seen = ctx?.signal;
        return "x";
      },
    };
    const ctrl = new AbortController();

    await step(
      { messages: [{ role: "user", content: "x" }] },
      { complete, tools: [probe], signal: ctrl.signal },
    );

    expect(seen).toBe(ctrl.signal);
  });

  test("keeps a committed step when the signal aborts after the tool runs", async () => {
    const ctrl = new AbortController();
    const kinds: string[] = [];
    const complete: ModelClient = async () => ({
      role: "assistant",
      content: "",
      tool_calls: [{ id: "c1", name: "echo", arguments: "{}" }],
    });
    const echo: Tool = {
      name: "echo",
      description: "echo",
      parameters: {},
      execute: () => {
        ctrl.abort();
        return "x";
      },
    };

    const next = await runUntilIdle(
      { messages: [{ role: "user", content: "x" }] },
      {
        complete,
        tools: [echo],
        signal: ctrl.signal,
        agentId: "agent-1",
        session: "session-1",
        memory: {
          append: (input) => {
            kinds.push(input.kind);
            return {
              v: 1,
              id: "01TEST",
              seq: kinds.length,
              ts: "",
              ...input,
            };
          },
        },
      },
    );

    expect(kinds).toEqual(["action", "observation"]);
    expect(next.messages.at(-1)).toEqual({
      role: "tool",
      tool_call_id: "c1",
      content: "x",
    });
  });

  test("returns the last committed state when complete aborts before admit", async () => {
    const ctrl = new AbortController();
    const start: AgentState = { messages: [{ role: "user", content: "x" }] };
    const complete: ModelClient = async () => {
      ctrl.abort();
      throw new Error("turn cancelled");
    };

    const next = await runUntilIdle(start, {
      complete,
      tools: [],
      signal: ctrl.signal,
    });

    expect(next).toBe(start);
  });
});

describe("step memory", () => {
  test("publishes action and observation as separate records", async () => {
    const published: { kind: string; session?: string; from_kind: string; tags: readonly unknown[] }[] =
      [];
    const complete: ModelClient = async () => ({
      role: "assistant",
      content: "using echo",
      tool_calls: [{ id: "c1", name: "echo", arguments: JSON.stringify({ text: "z" }) }],
    });
    const echo: Tool = {
      name: "echo",
      description: "echo",
      parameters: {},
      execute: (args) => String(args.text),
    };

    await step(
      { messages: [{ role: "user", content: "z" }] },
      {
        complete,
        tools: [echo],
        agentId: "agent-1",
        session: "session-1",
        memory: {
          append: (input) => {
            published.push(input);
            return {
              v: 1,
              id: "01TEST",
              seq: published.length,
              ts: "",
              ...input,
            };
          },
        },
      },
    );

    expect(published.map((row) => row.kind)).toEqual(["utterance", "action", "observation"]);
    expect(published.every((row) => row.session === "session-1")).toBe(true);
    expect(published.every((row) => row.from_kind === "agent")).toBe(true);
    expect(published.every((row) => row.tags.length === 0)).toBe(true);
  });
});


