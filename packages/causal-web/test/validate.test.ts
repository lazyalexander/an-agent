import { describe, expect, test } from "bun:test";
import { parseTape, type Memevent } from "../src/types";
import { ulidTimeMs, validateTape } from "../src/validate";

const text = await Bun.file(new URL("../examples/sample.jsonl", import.meta.url)).text();
const tape = parseTape(text);
const failedOf = (events: Memevent[]) =>
  validateTape({ events, parseErrors: [] })
    .filter((c) => !c.ok)
    .map((c) => c.id);

describe("sample tape", () => {
  test("parses and passes every check", () => {
    expect(tape.parseErrors).toEqual([]);
    expect(failedOf(tape.events)).toEqual([]);
  });
  test("ulid time decodes to the event ts", () => {
    const e = tape.events[0];
    expect(ulidTimeMs(e.id)).toBe(Date.parse(e.ts));
  });
});

describe("mutations fail the right check", () => {
  test("dropping observations breaks action-pairing", () => {
    const renumbered = tape.events
      .filter((e) => e.kind !== "observation")
      .map((e, i) => ({ ...e, seq: i + 1 }));
    expect(failedOf(renumbered)).toContain("action-pairing");
  });
  test("a dangling ref breaks refs-resolve", () => {
    const events = tape.events.map((e) => ({ ...e }));
    events[3] = { ...events[3], refs: ["01ZZZZZZZZZZZZZZZZZZZZZZZZ"] };
    expect(failedOf(events)).toContain("refs-resolve");
  });
  test("reordered lines break seq-gapless", () => {
    const events = [...tape.events];
    [events[1], events[2]] = [events[2], events[1]];
    expect(failedOf(events)).toContain("seq-gapless");
  });
  test("tampered ts breaks ulid-time", () => {
    const events = tape.events.map((e) => ({ ...e }));
    events[4] = { ...events[4], ts: new Date(Date.parse(events[4].ts) + 60_000).toISOString() };
    expect(failedOf(events)).toContain("ulid-time");
  });
  test("a corrupt line breaks parse", () => {
    const bad = parseTape(text + "not-json\n");
    const failed = validateTape(bad)
      .filter((c) => !c.ok)
      .map((c) => c.id);
    expect(failed).toContain("parse");
  });
});
