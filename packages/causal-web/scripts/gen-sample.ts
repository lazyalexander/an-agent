// Regenerate examples/sample.jsonl: bun scripts/gen-sample.ts
// Fixture ids are real ULIDs whose embedded time equals the event ts,
// so the ulid-time check passes (mirroring det_seam::Entropy::ulid).

const A = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

function ulid(ms: number, rnd: number): string {
  let s = "";
  let t = ms;
  for (let i = 0; i < 10; i++) {
    s = A[t % 32] + s;
    t = Math.floor(t / 32);
  }
  let r = rnd >>> 0;
  for (let i = 0; i < 16; i++) {
    s += A[r % 32];
    r = Math.floor(r / 32);
  }
  return s;
}

const BARE = { form: "bare", permit: "go", file: "none", memory: "ignore" };
const EFFECT = { reads: [], writes: [], unbounded: false, memory: "ignore" };
const session = "11111111-2222-3333-4444-555555555555";
const base = Date.parse("2026-09-16T10:00:00.000Z");

let seq = 0;
const events: object[] = [];

function ev(
  ms: number,
  fromKind: string,
  kind: string,
  content: string,
  extra: object = {},
): string {
  seq += 1;
  const t = base + ms;
  const id = ulid(t, (seq * 2654435761) % 2 ** 32);
  events.push({
    v: 1,
    id,
    seq,
    ts: new Date(t).toISOString(),
    from: "demo-agent",
    from_kind: fromKind,
    kind,
    session,
    content,
    tags: [],
    refs: [],
    ...extra,
  });
  return id;
}

ev(
  0,
  "human",
  "utterance",
  "Who created the Rust programming language, and at which company? Search the web, remember the key fact, then answer.",
);
ev(900, "agent", "utterance", "I'll search for this.", {
  act: { kind: "utterance", tag: BARE },
});
const a1 = ev(901, "agent", "action", 'web_search {"query": "Rust creator company"}', {
  act: { kind: "tool", tag: BARE, tool: "web_search" },
});
ev(
  2400,
  "agent",
  "observation",
  "Rust (programming language): Graydon Hoare created Rust in 2006 while working at Mozilla, which officially sponsored the project in 2009.",
  { refs: [a1], act: { kind: "tool", tag: BARE, tool: "web_search", effect: EFFECT } },
);
ev(3100, "agent", "utterance", "Confirmed. Storing the fact.", {
  act: { kind: "utterance", tag: BARE },
});
const a2 = ev(
  3101,
  "agent",
  "action",
  'remember {"text": "Rust was created by Graydon Hoare in 2006 while working at Mozilla; Mozilla sponsored the project from 2009."}',
  { act: { kind: "tool", tag: BARE, tool: "remember" } },
);
const clip = ev(
  3101,
  "agent",
  "utterance",
  "Rust was created by Graydon Hoare in 2006 while working at Mozilla; Mozilla sponsored the project from 2009.",
  { tags: ["memory"], act: { kind: "remember", tag: BARE, tool: "remember" } },
);
ev(3102, "agent", "observation", `remembered as ${clip}`, {
  refs: [a2],
  act: { kind: "tool", tag: BARE, tool: "remember", effect: EFFECT },
});
ev(
  3900,
  "agent",
  "utterance",
  "Rust was created by Graydon Hoare in 2006, while working at Mozilla (sponsored from 2009). I've stored this fact.",
  { act: { kind: "utterance", tag: BARE } },
);

await Bun.write(
  new URL("../examples/sample.jsonl", import.meta.url),
  events.map((e) => JSON.stringify(e)).join("\n") + "\n",
);
console.log(`wrote ${events.length} events`);
