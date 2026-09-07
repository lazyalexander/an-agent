import { describe, expect, test } from "bun:test";
import { step, type AgentState, type AssistantMessage, type ModelClient } from "../src/agent.ts";

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
