import type { AssistantMessage, Message, ModelClient, Tool, ToolCall } from "./types.ts";

export type ChatCompletionsOptions = {
  apiKey?: string;
  apiKeyEnv?: string;
  model: string;
  baseUrl: string;
  fetch?: typeof fetch;
  extraBody?: Record<string, unknown>;
};

const toApiMessage = (message: Message): Record<string, unknown> => {
  if (message.role === "tool") {
    return {
      role: "tool",
      tool_call_id: message.tool_call_id,
      content: message.content,
    };
  }
  if (message.role === "assistant" && message.tool_calls && message.tool_calls.length > 0) {
    return {
      role: "assistant",
      content: message.content,
      tool_calls: message.tool_calls.map((call) => ({
        id: call.id,
        type: "function",
        function: { name: call.name, arguments: call.arguments },
      })),
    };
  }
  return { role: message.role, content: message.content };
};

const toApiTools = (tools: readonly Tool[]) =>
  tools.map((tool) => ({
    type: "function",
    function: {
      name: tool.name,
      description: tool.description,
      parameters: tool.parameters,
    },
  }));

const mapToolCalls = (raw: unknown): readonly ToolCall[] | undefined => {
  if (!Array.isArray(raw) || raw.length === 0) return undefined;
  return raw.map((item) => {
    const row = item as { id: string; function?: { name: string; arguments: string } };
    return {
      id: row.id,
      name: row.function?.name ?? "",
      arguments: row.function?.arguments ?? "{}",
    };
  });
};

export function createChatCompletionsClient(options: ChatCompletionsOptions): ModelClient {
  const baseUrl = options.baseUrl.replace(/\/$/, "");
  const fetchImpl = options.fetch ?? fetch;
  const extraBody = options.extraBody ?? {};

  return async ({ messages, tools }) => {
    const apiKey = options.apiKey ?? process.env[options.apiKeyEnv ?? "MODEL_API_KEY"];
    if (!apiKey) {
      throw new Error(`${options.apiKeyEnv ?? "MODEL_API_KEY"} is not set`);
    }

    const body: Record<string, unknown> = {
      model: options.model,
      messages: messages.map(toApiMessage),
      ...extraBody,
    };
    if (tools.length > 0) body.tools = toApiTools(tools);

    const response = await fetchImpl(`${baseUrl}/chat/completions`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${apiKey}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(body),
    });

    if (!response.ok) {
      throw new Error(`chat completions HTTP ${response.status}: ${await response.text()}`);
    }

    const json = (await response.json()) as {
      choices?: { message?: { content?: string | null; tool_calls?: unknown } }[];
    };
    const message = json.choices?.[0]?.message;
    const tool_calls = mapToolCalls(message?.tool_calls);
    const result: AssistantMessage = {
      role: "assistant",
      content: message?.content ?? "",
    };
    if (tool_calls) result.tool_calls = tool_calls;
    return result;
  };
}
