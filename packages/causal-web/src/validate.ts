import type { Memevent, ParsedTape } from "./types";

export interface Issue {
  where: string;
  message: string;
}

export interface Check {
  id: string;
  name: string;
  ok: boolean;
  issues: Issue[];
}

const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const ULID_RE = /^[0-9A-HJ-KM-NP-TV-Z]{26}$/;

/** Milliseconds embedded in a ULID's first 10 chars (48-bit big-endian), or null if not a ULID. */
export function ulidTimeMs(id: string): number | null {
  if (!ULID_RE.test(id)) return null;
  let ms = 0;
  for (let i = 0; i < 10; i++) ms = ms * 32 + CROCKFORD.indexOf(id[i]);
  return ms;
}

const KINDS = new Set(["utterance", "action", "observation"]);
const FROM_KINDS = new Set(["human", "agent", "unknown"]);

export function validateTape(tape: ParsedTape): Check[] {
  const { events, parseErrors } = tape;
  const checks: Check[] = [];
  const add = (id: string, name: string, issues: Issue[]) =>
    checks.push({ id, name, ok: issues.length === 0, issues });

  add(
    "parse",
    "every line parses as JSON",
    parseErrors.map((e) => ({ where: `line ${e.line}`, message: e.message })),
  );

  const schema: Issue[] = [];
  for (const e of events) {
    const w = `seq ${e?.seq ?? "?"}`;
    if (e.v !== 1) schema.push({ where: w, message: `v must be 1, got ${e.v}` });
    if (typeof e.id !== "string" || e.id.length === 0)
      schema.push({ where: w, message: "id missing" });
    if (!Number.isInteger(e.seq) || e.seq < 1)
      schema.push({ where: w, message: `seq must be a positive integer, got ${e.seq}` });
    if (Number.isNaN(Date.parse(e.ts)))
      schema.push({ where: w, message: `ts not parseable: ${e.ts}` });
    if (!KINDS.has(e.kind)) schema.push({ where: w, message: `unknown kind: ${e.kind}` });
    if (!FROM_KINDS.has(e.from_kind))
      schema.push({ where: w, message: `unknown from_kind: ${e.from_kind}` });
    if (!Array.isArray(e.tags) || !Array.isArray(e.refs))
      schema.push({ where: w, message: "tags/refs must be arrays" });
    if (typeof e.content !== "string")
      schema.push({ where: w, message: "content must be a string" });
  }
  add("schema", "schema v=1 fields", schema);

  const seq: Issue[] = [];
  events.forEach((e, i) => {
    if (e.seq !== i + 1)
      seq.push({ where: `line ${i + 1}`, message: `expected seq ${i + 1}, got ${e.seq}` });
  });
  add("seq-gapless", "seq is 1..n without gaps", seq);

  const uniq: Issue[] = [];
  const seen = new Map<string, number>();
  for (const e of events) {
    const prior = seen.get(e.id);
    if (prior !== undefined) uniq.push({ where: `seq ${e.seq}`, message: `id also used by seq ${prior}` });
    else seen.set(e.id, e.seq);
  }
  add("unique-ids", "event ids are unique", uniq);

  const shape: Issue[] = [];
  for (const e of events)
    if (ulidTimeMs(e.id) === null)
      shape.push({ where: `seq ${e.seq}`, message: `id is not a ULID: ${e.id}` });
  add("ulid-shape", "ids are ULIDs", shape);

  const time: Issue[] = [];
  for (const e of events) {
    const ms = ulidTimeMs(e.id);
    const ts = Date.parse(e.ts);
    if (ms === null || Number.isNaN(ts)) continue;
    if (ms !== ts) time.push({ where: `seq ${e.seq}`, message: `ULID time ${ms} != ts ${ts}` });
  }
  add("ulid-time", "ULID time matches ts", time);

  const mono: Issue[] = [];
  for (let i = 1; i < events.length; i++) {
    const prev = Date.parse(events[i - 1].ts);
    const cur = Date.parse(events[i].ts);
    if (!Number.isNaN(prev) && !Number.isNaN(cur) && cur < prev)
      mono.push({
        where: `seq ${events[i].seq}`,
        message: `ts goes backwards (after seq ${events[i - 1].seq})`,
      });
  }
  add("ts-monotonic", "ts is non-decreasing", mono);

  const ids = new Set(events.map((e) => e.id));
  const refs: Issue[] = [];
  for (const e of events)
    for (const r of e.refs)
      if (!ids.has(r)) refs.push({ where: `seq ${e.seq}`, message: `dangling ref ${r}` });
  add("refs-resolve", "refs point to existing events", refs);

  const byId = new Map(events.map((e) => [e.id, e]));
  const linkage: Issue[] = [];
  for (const e of events) {
    if (e.kind !== "observation") continue;
    if (e.refs.length === 0) {
      linkage.push({ where: `seq ${e.seq}`, message: "observation has no refs" });
      continue;
    }
    for (const r of e.refs) {
      const target = byId.get(r);
      if (target && target.kind !== "action")
        linkage.push({
          where: `seq ${e.seq}`,
          message: `ref points to ${target.kind} (seq ${target.seq}), expected action`,
        });
    }
  }
  add("observation-linkage", "observations cite actions", linkage);

  const pairing: Issue[] = [];
  for (const e of events) {
    if (e.kind !== "action" || !e.act?.tool) continue;
    const answered = events.some((o) => o.kind === "observation" && o.refs.includes(e.id));
    if (!answered)
      pairing.push({ where: `seq ${e.seq}`, message: `tool action "${e.act.tool}" has no observation` });
  }
  add("action-pairing", "every tool action is answered", pairing);

  const sessions = new Set(events.map((e) => e.session).filter((s) => s !== undefined));
  add(
    "single-session",
    "one session per tape",
    sessions.size <= 1
      ? []
      : [{ where: "tape", message: `${sessions.size} distinct sessions` }],
  );

  return checks;
}

export function score(checks: Check[]): { passed: number; total: number } {
  return { passed: checks.filter((c) => c.ok).length, total: checks.length };
}
