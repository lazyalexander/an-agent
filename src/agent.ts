export type {
  AgentDeps,
  AgentState,
  AssistantMessage,
  Message,
  ModelClient,
  Tool,
  ToolCall,
} from "./types.ts";

import type { AgentDeps, AgentState } from "./types.ts";

export async function step(state: AgentState, deps: AgentDeps): Promise<AgentState> {
  const assistant = await deps.complete({
    messages: state.messages,
    tools: deps.tools,
  });
  return { messages: [...state.messages, assistant] };
}
