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

export const lastAssistantText = (state: AgentState): string => {
  const last = state.messages.at(-1);
  return last && last.role === "assistant" ? last.content : "";
};
