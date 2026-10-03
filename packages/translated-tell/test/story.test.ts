import { describe, expect, test } from "bun:test";
import { FIXTURE } from "../src/fixture";
import { claims, frame } from "../src/story";
import { parseTape } from "../src/tape";

const tape = parseTape(FIXTURE);

describe("translated tell fixture", () => {
  test("parses the probe tape", () => {
    expect(tape.parseErrors).toEqual([]);
    expect(tape.events.length).toBeGreaterThan(20);
  });

  test("every claim holds on the full tape and not on the first event", () => {
    expect(claims.every((claim) => claim.holds(tape.events))).toBe(true);
    expect(claims.some((claim) => claim.holds(tape.events.slice(0, 1)))).toBe(false);
  });

  test("the last frame still has the written line and an empty mount for editor", () => {
    const end = frame(tape.events);
    expect(end.editor).toEqual(["译:hello"]);
    expect(end.payload).toBe("译:hello");
    expect(end.mounted).toEqual([]);
  });
});
