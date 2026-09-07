import type { AgentState } from "./types.ts";

export type LineCommand =
  | { kind: "exit" }
  | { kind: "skip" }
  | { kind: "user"; text: string };

export const parseLine = (line: string): LineCommand => {
  const text = line.trim();
  if (text === "") return { kind: "skip" };
  if (text === "/exit") return { kind: "exit" };
  return { kind: "user", text };
};

export const isInputEnded = (err: unknown): boolean => {
  if (!(err instanceof Error)) return false;
  const code = "code" in err ? String((err as { code?: unknown }).code) : "";
  if (code === "ERR_USE_AFTER_CLOSE" || code === "ABORT_ERR") return true;
  const msg = err.message.toLowerCase();
  return msg.includes("readline was closed") || msg.includes("cursor is closed");
};

export const readCommand = async (question: () => Promise<string>): Promise<LineCommand> => {
  try {
    return parseLine(await question());
  } catch (err) {
    if (isInputEnded(err)) return { kind: "exit" };
    throw err;
  }
};

export const lastAssistantText = (state: AgentState): string => {
  const last = state.messages.at(-1);
  return last && last.role === "assistant" ? last.content : "";
};
