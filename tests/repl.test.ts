import { describe, expect, test } from "bun:test";
import { lastAssistantText, parseLine } from "../src/repl.ts";

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
