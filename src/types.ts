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

export type Tool = {
  name: string;
  description: string;
  parameters: Record<string, unknown>;
  execute: (args: Record<string, unknown>) => Promise<string> | string;
};

export type ModelClient = (input: {
  messages: readonly Message[];
  tools: readonly Tool[];
}) => Promise<AssistantMessage>;

export type AgentState = {
  readonly messages: readonly Message[];
};

export type AgentDeps = {
  complete: ModelClient;
  tools: readonly Tool[];
  ops?: import("./ops.ts").Ops;
};
