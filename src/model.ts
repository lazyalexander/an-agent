import { createChatCompletionsClient } from "./chat-completions.ts";
import { createDeepSeekClient } from "./deepseek.ts";
import type { ModelClient } from "./types.ts";

export type ModelEnvOptions = {
  fetch?: typeof fetch;
  env?: NodeJS.ProcessEnv;
};

export function createModelClient(options: ModelEnvOptions = {}): ModelClient {
  const env = options.env ?? process.env;
  const provider = (env.MODEL_PROVIDER ?? "deepseek").toLowerCase();

  if (provider === "deepseek") {
    return createDeepSeekClient({
      fetch: options.fetch,
      apiKey: env.DEEPSEEK_API_KEY,
      model: env.DEEPSEEK_MODEL,
    });
  }

  const baseUrl = env.MODEL_BASE_URL;
  const model = env.MODEL_NAME;
  if (!baseUrl || !model) {
    throw new Error("MODEL_BASE_URL and MODEL_NAME are required when MODEL_PROVIDER is not deepseek");
  }

  return createChatCompletionsClient({
    apiKey: env.MODEL_API_KEY,
    model,
    baseUrl,
    fetch: options.fetch,
  });
}
