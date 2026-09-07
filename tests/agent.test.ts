import { describe, expect, test } from "bun:test";
import {
  step,
  type AgentState,
  type AssistantMessage,
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
