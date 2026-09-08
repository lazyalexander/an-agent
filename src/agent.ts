import type { Kind } from "./memory/index.ts";
import { timed } from "./ops.ts";
import type { AgentDeps, AgentState, Tool, ToolCall, ToolMessage } from "./types.ts";

// Only path from the agent loop onto the tape. Call this in the same breath as
// putting the matching message into the returned state; never throw that state away.
const admit = (
  deps: AgentDeps,
  kind: Kind,
  content: string,
): void => {
  if (!deps.memory || !deps.agentId || !deps.session) return;
  deps.memory.append({
    from: deps.agentId,
    from_kind: "agent",
    kind,
    session: deps.session,
    tags: [],
    content,
    refs: [],
  });
};

export type {
  AgentDeps,
  AgentState,
  AssistantMessage,
  Message,
  ModelClient,
  Tool,
  ToolCall,
} from "./types.ts";

type ParseResult =
  | { ok: true; value: Record<string, unknown> }
  | { ok: false; error: string };

const parseArgs = (raw: string): ParseResult => {
  try {
    const value: unknown = JSON.parse(raw);
    if (value !== null && typeof value === "object" && !Array.isArray(value)) {
      return { ok: true, value: value as Record<string, unknown> };
    }
    return { ok: false, error: "tool arguments must be a JSON object" };
  } catch {
    return { ok: false, error: "tool arguments are not valid JSON" };
  }
};

const runTool = async (
  call: ToolCall,
  tools: readonly Tool[],
  deps: AgentDeps,
): Promise<ToolMessage> => {
  const tool = tools.find((t) => t.name === call.name);
  if (!tool) {
    return {
      role: "tool",
      tool_call_id: call.id,
      content: `unknown tool: ${call.name}`,
    };
  }
  const parsed = parseArgs(call.arguments);
  if (!parsed.ok) {
    return { role: "tool", tool_call_id: call.id, content: parsed.error };
  }
  try {
    const run = () => Promise.resolve(tool.execute(parsed.value, { signal: deps.signal }));
    const content = deps.ops ? await timed(deps.ops, `tool.${call.name}`, run) : await run();
    return { role: "tool", tool_call_id: call.id, content };
  } catch (err) {
    return {
      role: "tool",
      tool_call_id: call.id,
      content: err instanceof Error ? err.message : String(err),
    };
  }
};

// One model call. Input state is not mutated; tool failures become tool messages, not throws.
export async function step(state: AgentState, deps: AgentDeps): Promise<AgentState> {
  const assistant = await deps.complete({
    messages: state.messages,
    tools: deps.tools,
    signal: deps.signal,
  });
  if (assistant.content) admit(deps, "utterance", assistant.content);
  const messages = [...state.messages, assistant];
  const calls = assistant.tool_calls ?? [];
  if (calls.length === 0) {
    return { messages };
  }
  const toolMessages: ToolMessage[] = [];
  for (const call of calls) {
    admit(deps, "action", `${call.name} ${call.arguments}`);
    const result = await runTool(call, deps.tools, deps);
    admit(deps, "observation", result.content);
    toolMessages.push(result);
  }
  return { messages: [...messages, ...toolMessages] };
}

const lastIsFinalAssistant = (state: AgentState): boolean => {
  const last = state.messages.at(-1);
  if (!last || last.role !== "assistant") return false;
  return (last.tool_calls ?? []).length === 0;
};

// Repeat step until the last message is an assistant with no tool_calls.
// A step that returned has committed (tape + messages). Abort stops the next
// step; it does not throw away the one that already finished. If complete()
// aborts before admit, this returns the last committed state.
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
