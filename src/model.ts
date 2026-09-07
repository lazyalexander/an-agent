import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { createChatCompletionsClient } from "./chat-completions.ts";
import { packageRoot, type EnvMap } from "./load-env.ts";
import type { ModelClient } from "./types.ts";

export type ModelSettings = {
  baseUrl: string;
  model: string;
  extraBody?: Record<string, unknown>;
  apiKey?: string;
  apiKeyEnv?: string;
};

export type ModelClientOptions = {
  fetch?: typeof fetch;
  env?: EnvMap;
  settings?: ModelSettings;
  configPath?: string;
  ops?: import("./ops.ts").Ops;
};

type FileConfig = {
  baseUrl?: string;
  model?: string;
  extraBody?: Record<string, unknown>;
  apiKeyEnv?: string;
};

const readFileConfig = (path: string): FileConfig => {
  if (!existsSync(path)) return {};
  return JSON.parse(readFileSync(path, "utf8")) as FileConfig;
};

const parseExtraBody = (raw: string | undefined): Record<string, unknown> | undefined => {
  if (raw === undefined || raw === "") return undefined;
  const value: unknown = JSON.parse(raw);
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("MODEL_EXTRA_BODY must be a JSON object");
  }
  return value as Record<string, unknown>;
};

export function loadModelSettings(
  env: EnvMap = process.env,
  configPath = join(packageRoot(), "config", "model.json"),
): ModelSettings {
  const file = readFileConfig(configPath);
  const apiKeyEnv = file.apiKeyEnv ?? "MODEL_API_KEY";
  // Env overrides config/model.json. Secrets never live in the json file.
  const baseUrl = env.MODEL_BASE_URL ?? file.baseUrl;
  const model = env.MODEL_NAME ?? file.model;
  if (!baseUrl || !model) {
    throw new Error("model baseUrl and model name are required (config/model.json or MODEL_BASE_URL / MODEL_NAME)");
  }
  const extraBody = parseExtraBody(env.MODEL_EXTRA_BODY) ?? file.extraBody;
  const apiKey = env.MODEL_API_KEY ?? env[apiKeyEnv];
  return {
    baseUrl,
    model,
    extraBody,
    apiKeyEnv,
    ...(apiKey !== undefined ? { apiKey } : {}),
  };
}

export function createModelClient(options: ModelClientOptions = {}): ModelClient {
  const settings = options.settings ?? loadModelSettings(options.env ?? process.env, options.configPath);
  return createChatCompletionsClient({
    apiKey: settings.apiKey,
    apiKeyEnv: settings.apiKeyEnv,
    model: settings.model,
    baseUrl: settings.baseUrl,
    extraBody: settings.extraBody,
    fetch: options.fetch,
    ops: options.ops,
  });
}
