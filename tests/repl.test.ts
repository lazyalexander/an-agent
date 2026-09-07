import { describe, expect, test } from "bun:test";
import { isInputEnded, lastAssistantText, parseLine, readCommand } from "../src/repl.ts";

describe("parseLine", () => {
  test("treats blank input as skip", () => {
    expect(parseLine("  ")).toEqual({ kind: "skip" });
  });

  test("treats /exit as exit", () => {
    expect(parseLine("/exit")).toEqual({ kind: "exit" });
  });

  test("treats other text as a user message", () => {
    expect(parseLine(" hello ")).toEqual({ kind: "user", text: "hello" });
  });
});

describe("lastAssistantText", () => {
  test("returns the last assistant content", () => {
    expect(
      lastAssistantText({
        messages: [
          { role: "user", content: "hi" },
          { role: "assistant", content: "there" },
        ],
      }),
    ).toBe("there");
  });
});

describe("isInputEnded", () => {
  test("treats readline-closed and EOF as input end", () => {
    expect(isInputEnded(new Error("readline was closed"))).toBe(true);
    expect(isInputEnded(new Error("The cursor is closed"))).toBe(true);
    const eof = new Error("ended") as Error & { code?: string };
    eof.code = "ERR_USE_AFTER_CLOSE";
    expect(isInputEnded(eof)).toBe(true);
    expect(isInputEnded(new Error("chat completions HTTP 500"))).toBe(false);
  });

  test("readCommand maps a closed readline to exit", async () => {
    const command = await readCommand(async () => {
      throw new Error("readline was closed");
    });
    expect(command).toEqual({ kind: "exit" });
  });
});

