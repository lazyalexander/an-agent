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

describe("step logging", () => {
  test("records assistant, tool_call, and tool_result in order", async () => {
    const events: { type: string }[] = [];
    const complete: ModelClient = async () => ({
      role: "assistant",
      content: "",
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
        log: { append: (event) => events.push(event) },
      },
    );

    expect(events.map((e) => e.type)).toEqual(["assistant", "tool_call", "tool_result"]);
  });
});

