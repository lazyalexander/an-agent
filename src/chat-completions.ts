import { timed } from "./ops.ts";
import type { AssistantMessage, Message, ModelClient, Tool, ToolCall } from "./types.ts";

export type ChatCompletionsOptions = {
  apiKey?: string;
  apiKeyEnv?: string;
  model: string;
  baseUrl: string;
  fetch?: typeof fetch;
  extraBody?: Record<string, unknown>; // vendor fields (e.g. thinking), merged into the JSON body
  ops?: import("./ops.ts").Ops;
  /** Per-attempt request timeout. Default 180s. */
  timeoutMs?: number;
  /** Total attempts including the first. Default 4. */
  maxAttempts?: number;
  /** Backoff base in ms; attempt N waits base * 2^(N-1) with jitter. Mainly for tests. */
  retryBaseMs?: number;
};

const DEFAULT_TIMEOUT_MS = 180_000;
const DEFAULT_MAX_ATTEMPTS = 4;
const DEFAULT_RETRY_BASE_MS = 1_000;
const RETRY_CAP_MS = 15_000;
const RETRY_AFTER_CAP_MS = 60_000;
const ERROR_BODY_EXCERPT = 500;

/** A failure the identical request may recover from on a later attempt. */
class RetryableError extends Error {
  retryAfterMs?: number;
  constructor(message: string, retryAfterMs?: number) {
    super(message);
    this.retryAfterMs = retryAfterMs;
  }
}

const RETRYABLE_STATUS = new Set([408, 409, 425, 429]);

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

// The wire is untrusted. Repair what is safe to repair (a synthesized id keeps the
// assistant/tool pairing internally consistent); never let undefined leak into types.
const mapToolCalls = (
  raw: unknown,
  repair: (note: string) => void,
): readonly ToolCall[] | undefined => {
  if (!Array.isArray(raw) || raw.length === 0) return undefined;
  return raw.map((item, index) => {
    const row = (item ?? {}) as { id?: unknown; function?: unknown };
    const fn = (row.function ?? {}) as { name?: unknown; arguments?: unknown };
    let id = typeof row.id === "string" && row.id !== "" ? row.id : undefined;
    if (id === undefined) {
      id = `call_${index}_${crypto.randomUUID().slice(0, 8)}`;
      repair(`tool_calls[${index}] had no id; synthesized ${id}`);
    }
    const name = typeof fn.name === "string" ? fn.name : "";
    let args: string;
    if (typeof fn.arguments === "string") {
      args = fn.arguments;
    } else if (fn.arguments === undefined || fn.arguments === null) {
      args = "{}";
    } else {
      args = JSON.stringify(fn.arguments);
      repair(`tool_calls[${index}] arguments were not a string; re-serialized`);
    }
    return { id, name, arguments: args };
  });
};

// Validate the response shape at the boundary. "No choices" is often gateway
// flakiness, so it is retryable; a choice without a message is a hard violation.
const parseAssistant = (json: unknown, repair: (note: string) => void): AssistantMessage => {
  const root = (json ?? {}) as { choices?: unknown };
  if (!Array.isArray(root.choices) || root.choices.length === 0) {
    throw new RetryableError("malformed chat completion response: no choices");
  }
  const choice = root.choices[0] as { message?: unknown; finish_reason?: unknown };
  if (choice.message === null || typeof choice.message !== "object") {
    if (choice.finish_reason === "content_filter") {
      throw new Error("chat completion blocked by provider content filter");
    }
    throw new Error("malformed chat completion response: choice has no message");
  }
  const message = choice.message as { content?: unknown; tool_calls?: unknown };
  const result: AssistantMessage = {
    role: "assistant",
    content: typeof message.content === "string" ? message.content : "",
  };
  const tool_calls = mapToolCalls(message.tool_calls, repair);
  if (tool_calls) result.tool_calls = tool_calls;
  return result;
};

const retryAfterMs = (headers: Headers): number | undefined => {
  const raw = headers.get("retry-after");
  if (!raw) return undefined;
  const seconds = Number(raw);
  if (Number.isFinite(seconds)) return Math.max(0, seconds * 1000);
  const at = Date.parse(raw);
  return Number.isNaN(at) ? undefined : Math.max(0, at - Date.now());
};

const backoffMs = (attempt: number, baseMs: number, serverHintMs?: number): number => {
  if (serverHintMs !== undefined) return Math.min(serverHintMs, RETRY_AFTER_CAP_MS);
  const exp = Math.min(baseMs * 2 ** (attempt - 1), RETRY_CAP_MS);
  return Math.round(exp * (0.5 + Math.random() * 0.5));
};

const sleep = (ms: number, signal?: AbortSignal): Promise<void> =>
  new Promise((resolve, reject) => {
    if (signal?.aborted) {
      reject(new Error("turn cancelled"));
      return;
    }
    const onAbort = () => {
      clearTimeout(timer);
      reject(new Error("turn cancelled"));
    };
    const timer = setTimeout(() => {
      signal?.removeEventListener("abort", onAbort);
      resolve();
    }, ms);
    signal?.addEventListener("abort", onAbort, { once: true });
  });

export function createChatCompletionsClient(options: ChatCompletionsOptions): ModelClient {
  const baseUrl = options.baseUrl.replace(/\/$/, "");
  const fetchImpl = options.fetch ?? fetch;
  const extraBody = options.extraBody ?? {};
  const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const maxAttempts = Math.max(1, options.maxAttempts ?? DEFAULT_MAX_ATTEMPTS);
  const retryBaseMs = options.retryBaseMs ?? DEFAULT_RETRY_BASE_MS;

  return async ({ messages, tools, signal }) => {
    const attemptOnce = async (): Promise<AssistantMessage> => {
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

      const timeout = AbortSignal.timeout(timeoutMs);
      const requestSignal = signal ? AbortSignal.any([timeout, signal]) : timeout;
      let response: Response;
      try {
        response = await fetchImpl(`${baseUrl}/chat/completions`, {
          method: "POST",
          headers: {
            Authorization: `Bearer ${apiKey}`,
            "Content-Type": "application/json",
          },
          body: JSON.stringify(body),
          signal: requestSignal,
        });
      } catch (err) {
        if (signal?.aborted) throw new Error("turn cancelled");
        if (timeout.aborted) {
          throw new RetryableError(`chat completions timed out after ${Math.round(timeoutMs / 1000)}s`);
        }
        throw new RetryableError(
          `chat completions request failed: ${err instanceof Error ? err.message : String(err)}`,
        );
      }

      if (!response.ok) {
        const excerpt = (await response.text()).slice(0, ERROR_BODY_EXCERPT);
        const detail = excerpt ? `: ${excerpt}` : "";
        if (RETRYABLE_STATUS.has(response.status) || response.status >= 500) {
          throw new RetryableError(
            `chat completions HTTP ${response.status}${detail}`,
            retryAfterMs(response.headers),
          );
        }
        throw new Error(`chat completions HTTP ${response.status}${detail}`);
      }

      let json: unknown;
      try {
        json = await response.json();
      } catch {
        throw new RetryableError("chat completions response body is not valid JSON");
      }
      const repairs: string[] = [];
      const result = parseAssistant(json, (note) => repairs.push(note));
      if (repairs.length > 0 && options.ops) {
        options.ops.record({
          kind: "error",
          level: "info",
          name: "model.complete.repair",
          error: repairs.join("; "),
        });
      }
      return result;
    };

    // Transient failures (network, timeout, 408/409/425/429/5xx, unparseable body,
    // empty choices) are retried with backoff so the user never sees them; anything
    // else fails fast with a clear message. An external abort never retries.
    const run = async (): Promise<AssistantMessage> => {
      for (let attempt = 1; ; attempt += 1) {
        if (signal?.aborted) throw new Error("turn cancelled");
        try {
          return await attemptOnce();
        } catch (err) {
          if (signal?.aborted) throw new Error("turn cancelled");
          if (!(err instanceof RetryableError)) throw err;
          if (attempt >= maxAttempts) {
            throw new Error(`chat completions failed after ${maxAttempts} attempts: ${err.message}`);
          }
          const waitMs = backoffMs(attempt, retryBaseMs, err.retryAfterMs);
          options.ops?.record({
            kind: "error",
            level: "info",
            name: "model.complete.retry",
            error: `attempt ${attempt}/${maxAttempts} failed: ${err.message}; retrying in ${waitMs}ms`,
          });
          await sleep(waitMs, signal);
        }
      }
    };

    return options.ops ? timed(options.ops, "model.complete", run) : run();
  };
}
