import { admitUtterance, runToolAct } from "./act/index.ts";
import { makeArray } from "./bedrock/opkit/array.ts";
import type { AgentDeps, AgentState } from "./types.ts";

export type {
  AgentDeps,
  AgentState,
  AssistantMessage,
  Message,
  ModelClient,
  Tool,
  ToolCall,
} from "./types.ts";

// One model call. Input state is not mutated; tool failures become tool messages, not throws.
export async function step(state: AgentState, deps: AgentDeps): Promise<AgentState> {
  const assistant = await deps.complete({
    messages: state.messages,
    tools: deps.tools,
    signal: deps.signal,
  });
  if (assistant.content) admitUtterance(deps, assistant.content);
  const messages = [...state.messages, assistant];
  const calls = makeArray(assistant.tool_calls ?? []);
  if (calls.length === 0) {
    return { messages };
  }
  const toolMessages = [];
  for (const call of calls) {
    const { message } = await runToolAct(deps, call);
    toolMessages.push(message);
  }
  return { messages: [...messages, ...toolMessages] };
}

const lastIsFinalAssistant = (state: AgentState): boolean => {
  const last = state.messages.at(-1);
  if (!last || last.role !== "assistant") return false;
  return (last.tool_calls ?? []).length === 0;
};

// Repeat step until the last message is an assistant with no tool_calls.
// A step that returned has committed (memstream + messages). Abort stops the
// next step; it does not throw away the one that already finished. If
// complete() aborts before admit, this returns the last committed state.
export async function runUntilIdle(
  state: AgentState,
  deps: AgentDeps,
  maxSteps = 16,
): Promise<AgentState> {
  let current = state;
  for (let i = 0; i < maxSteps; i += 1) {
    let next: AgentState;
    try {
      next = await step(current, deps);
    } catch (err) {
      if (deps.signal?.aborted) return current;
      throw err;
    }
    current = next;
    if (lastIsFinalAssistant(current)) return current;
    if (deps.signal?.aborted) return current;
  }
  throw new Error("agent exceeded maxSteps");
}
