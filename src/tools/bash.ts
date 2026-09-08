import type { Tool } from "../types.ts";

const DEFAULT_TIMEOUT_MS = 120_000;
const DEFAULT_MAX_OUTPUT_BYTES = 64 * 1024;
const DEFAULT_KILL_GRACE_MS = 1_000;
const FLUSH_GRACE_MS = 100;

export type BashOptions = {
  timeoutMs?: number;
  maxOutputBytes?: number;
  killGraceMs?: number;
};

type StreamResult = { text: string; truncated: boolean };

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

// Drain the pipe so the child cannot block on a full buffer; keep at most `cap` bytes.
const startCappedRead = (stream: ReadableStream<Uint8Array>, cap: number) => {
  const reader = stream.getReader();
  let stopped = false;
  const promise: Promise<StreamResult> = (async () => {
    const chunks: Uint8Array[] = [];
    let size = 0;
    let truncated = false;
    try {
      for (;;) {
        const { done, value } = await reader.read();
        if (done || stopped) break;
        if (size + value.length > cap) {
          const keep = cap - size;
          if (keep > 0) chunks.push(value.subarray(0, keep));
          size = cap;
          truncated = true;
        } else {
          chunks.push(value);
          size += value.length;
        }
      }
    } catch {
      /* cancelled or pipe closed */
    }
    const bytes = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) {
      bytes.set(chunk, offset);
      offset += chunk.length;
    }
    return { text: new TextDecoder().decode(bytes), truncated };
  })();
  return {
    promise,
    cancel: () => {
      if (stopped) return;
      stopped = true;
      void reader.cancel().catch(() => {});
    },
  };
};

const killGroup = (proc: { pid: number; kill: (signal?: NodeJS.Signals) => void }, signal: NodeJS.Signals) => {
  try {
    if (process.platform === "win32") proc.kill(signal);
    else process.kill(-proc.pid, signal);
  } catch {
    try {
      proc.kill(signal);
    } catch {
      /* already gone */
    }
  }
};

export const createBash = (options: BashOptions = {}): Tool => {
  const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const maxOutputBytes = options.maxOutputBytes ?? DEFAULT_MAX_OUTPUT_BYTES;
  const killGraceMs = options.killGraceMs ?? DEFAULT_KILL_GRACE_MS;

  return {
    name: "bash",
    description: `Run a shell command. Killed after ${Math.round(timeoutMs / 1000)}s; output truncated at ${maxOutputBytes} bytes. Arguments: { command: string }.`,
    parameters: {
      type: "object",
      properties: {
        command: { type: "string", description: "Shell command to run" },
      },
      required: ["command"],
    },
    execute: async (args, ctx) => {
      const command = String(args.command ?? "");
      if (!command) return "command is required";
      if (ctx?.signal?.aborted) return "cancelled; process killed";

      // New session so the child is process-group leader; kill(-pid) covers grandchildren.
      const proc = Bun.spawn(["bash", "-c", command], {
        detached: true,
        stdin: "ignore",
        stdout: "pipe",
        stderr: "pipe",
      });
      const stdout = startCappedRead(proc.stdout, maxOutputBytes);
      const stderr = startCappedRead(proc.stderr, maxOutputBytes);

      let stop: "timeout" | "cancelled" | undefined;
      let graceTimer: ReturnType<typeof setTimeout> | undefined;
      const stopWith = (reason: "timeout" | "cancelled") => {
        if (stop) return;
        stop = reason;
        killGroup(proc, "SIGTERM");
        graceTimer = setTimeout(() => killGroup(proc, "SIGKILL"), killGraceMs);
      };
      const timeout = setTimeout(() => stopWith("timeout"), timeoutMs);
      const onAbort = () => stopWith("cancelled");
      ctx?.signal?.addEventListener("abort", onAbort, { once: true });

      let reaped = false;
      try {
        const code = await proc.exited;
        reaped = true;
        await sleep(FLUSH_GRACE_MS);
        stdout.cancel();
        stderr.cancel();
        const [out, err] = await Promise.all([stdout.promise, stderr.promise]);
        const parts: string[] = [];
        if (code !== 0) {
          if (stop === "timeout") {
            parts.push(`timed out after ${Math.round(timeoutMs / 1000)}s; process killed`);
          } else if (stop === "cancelled") {
            parts.push("cancelled; process killed");
          } else {
            parts.push(`exit ${code}`);
          }
        }
        if (out.text) parts.push(out.text);
        if (out.truncated) parts.push("[stdout truncated]");
        if (err.text) parts.push(`[stderr]\n${err.text}`);
        if (err.truncated) parts.push("[stderr truncated]");
        return parts.join("\n");
      } finally {
        clearTimeout(timeout);
        if (graceTimer) clearTimeout(graceTimer);
        ctx?.signal?.removeEventListener("abort", onAbort);
        stdout.cancel();
        stderr.cancel();
        if (!reaped) {
          killGroup(proc, "SIGKILL");
          await proc.exited.catch(() => {});
        }
      }
    },
  };
};

export const bash = createBash();
