import { createChatCompletionsClient, type ChatCompletionsOptions } from "./chat-completions.ts";
import type { ModelClient } from "./types.ts";

export type DeepSeekOptions = Omit<ChatCompletionsOptions, "model" | "baseUrl"> & {
  model?: string;
  baseUrl?: string;
};

export function createDeepSeekClient(options: DeepSeekOptions = {}): ModelClient {
  return createChatCompletionsClient({
    ...options,
    apiKeyEnv: options.apiKeyEnv ?? "DEEPSEEK_API_KEY",
    apiKey: options.apiKey ?? process.env.DEEPSEEK_API_KEY,
    model: options.model ?? process.env.DEEPSEEK_MODEL ?? "deepseek-v4-flash",
    baseUrl: options.baseUrl ?? "https://api.deepseek.com",
    extraBody: { thinking: { type: "disabled" }, ...options.extraBody },
  });
}
