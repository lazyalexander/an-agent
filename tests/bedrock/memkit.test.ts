import { describe, expect, test } from "bun:test";
import { eventEqual, isOpenAction, sessionOf } from "../../src/bedrock/memkit/index.ts";
import { jsonCanonical, jsonCodec, jsonLine } from "../../src/bedrock/memkit/json.ts";
import { assertEvent, type MemoryEvent } from "../../src/bedrock/memkit/types.ts";

const event = (over: Partial<MemoryEvent> = {}): MemoryEvent => ({
  v: 1,
  id: "01TEST",
  seq: 1,
  ts: "2026-01-01T00:00:00.000Z",
  from: "agent-1",
  from_kind: "agent",
  kind: "action",
  session: "session-1",
  content: "echo {}",
  tags: [],
  refs: [],
  ...over,
});

describe("assertEvent", () => {
  test("accepts a well-formed event and rejects a broken one", () => {
    expect(assertEvent(event()).id).toBe("01TEST");
    expect(() => assertEvent({ v: 1 })).toThrow();
  });
});

describe("json adapter", () => {
  test("codec round-trips and canonical ignores key order", () => {
    const a = event();
    const round = jsonCodec.decode(jsonCodec.encode(a));
    expect(round).toEqual(a);
    const shuffled: MemoryEvent = {
      refs: a.refs,
      tags: a.tags,
      content: a.content,
      session: a.session,
      kind: a.kind,
      from_kind: a.from_kind,
      from: a.from,
      ts: a.ts,
      seq: a.seq,
      id: a.id,
      v: 1,
    };
    expect(eventEqual(a, shuffled, jsonCanonical)).toBe(true);
    const line = jsonLine.encode(a);
    expect(line.endsWith("\n")).toBe(true);
    expect(jsonLine.decode(line.trim()).id).toBe("01TEST");
  });
});

describe("envelope", () => {
  test("open action until an observation refs it", () => {
    const action = event({ id: "act-1", kind: "action" });
    const seen = event({
      id: "obs-1",
      seq: 2,
      kind: "observation",
      content: "ok",
      refs: ["act-1"],
    });
    expect(isOpenAction([action], "act-1")).toBe(true);
    expect(isOpenAction([action, seen], "act-1")).toBe(false);
    expect(sessionOf(action)).toBe("session-1");
  });
});
