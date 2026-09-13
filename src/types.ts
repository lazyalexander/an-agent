export type ToolCall = {
  id: string;
  name: string;
  arguments: string;
};

export type SystemMessage = {
  role: "system";
  content: string;
};

export type UserMessage = {
  role: "user";
  content: string;
};

export type AssistantMessage = {
  role: "assistant";
  content: string;
  tool_calls?: readonly ToolCall[];
};

export type ToolMessage = {
  role: "tool";
  tool_call_id: string;
  content: string;
};

export type Message = SystemMessage | UserMessage | AssistantMessage | ToolMessage;

import type { Tool, ToolContext } from "./tool/core.ts";
export type { Tool, ToolContext };

export type ModelClient = (input: {
  messages: readonly Message[];
  tools: readonly Tool[];
  signal?: AbortSignal;
}) => Promise<AssistantMessage>;

export type AgentState = {
  readonly messages: readonly Message[];
};

export type AgentDeps = {
  complete: ModelClient;
  tools: readonly Tool[];
  signal?: AbortSignal;
  agentId?: string;
  session?: string;
  memory?: { append: import("./memory/index.ts").MemoryStore["append"] };
  ops?: import("./ops.ts").Ops;
};
