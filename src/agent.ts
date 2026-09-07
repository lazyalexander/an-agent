import type { AgentDeps, AgentState, Tool, ToolCall, ToolMessage } from "./types.ts";

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

const runTool = async (call: ToolCall, tools: readonly Tool[]): Promise<ToolMessage> => {
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
    const content = await tool.execute(parsed.value);
    return { role: "tool", tool_call_id: call.id, content };
  } catch (err) {
    return {
      role: "tool",
      tool_call_id: call.id,
      content: err instanceof Error ? err.message : String(err),
    };
  }
};

export async function step(state: AgentState, deps: AgentDeps): Promise<AgentState> {
  const assistant = await deps.complete({
    messages: state.messages,
    tools: deps.tools,
  });
  const messages = [...state.messages, assistant];
  const calls = assistant.tool_calls ?? [];
  if (calls.length === 0) {
    return { messages };
  }
  const toolMessages = await Promise.all(calls.map((call) => runTool(call, deps.tools)));
  return { messages: [...messages, ...toolMessages] };
}
