import type { Memevent } from "./types";
import { score, type Check } from "./validate";

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  cls?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

function shortTime(ts: string): string {
  const t = ts.split("T")[1];
  return t ? t.replace("Z", "") : ts;
}

export function renderSummary(
  root: HTMLElement,
  fileName: string,
  events: Memevent[],
  checks: Check[],
): void {
  root.replaceChildren();
  const s = score(checks);
  const ok = s.passed === s.total;
  const head = el("div", ok ? "score ok" : "score bad");
  head.append(
    el("strong", undefined, `${s.passed}/${s.total}`),
    document.createTextNode(" checks · "),
    el("strong", undefined, `${events.length}`),
    document.createTextNode(` events · ${fileName}`),
  );
  root.append(head);
  if (events.length > 0) {
    const first = events[0];
    const last = events[events.length - 1];
    root.append(
      el(
        "div",
        "meta",
        `${shortTime(first.ts)} → ${shortTime(last.ts)} · session ${first.session ?? "(none)"}`,
      ),
    );
  }
}

export function renderChecks(
  root: HTMLElement,
  checks: Check[],
  onJump: (seq: number) => void,
): void {
  root.replaceChildren();
  for (const c of checks) {
    const row = el("div", c.ok ? "check ok" : "check bad");
    row.append(
      el("span", "mark", c.ok ? "✓" : "✗"),
      el("span", "name", c.name),
      el("span", "count", c.ok ? "" : `${c.issues.length}`),
    );
    if (!c.ok) {
      const list = el("ul", "issues");
      for (const issue of c.issues.slice(0, 8)) {
        const item = el("li");
        const m = /seq (\d+)/.exec(issue.where);
        if (m) {
          const a = el("a", "jump", issue.where);
          a.href = `#seq-${m[1]}`;
          a.addEventListener("click", (ev) => {
            ev.preventDefault();
            onJump(Number(m[1]));
          });
          item.append(a, document.createTextNode(` ${issue.message}`));
        } else {
          item.textContent = `${issue.where} ${issue.message}`;
        }
        list.append(item);
      }
      if (c.issues.length > 8) list.append(el("li", undefined, `… ${c.issues.length - 8} more`));
      row.append(list);
    }
    root.append(row);
  }
}

export function renderThread(root: HTMLElement, events: Memevent[]): void {
  root.replaceChildren();
  const citedBy = new Map<string, Memevent[]>();
  for (const e of events)
    for (const r of e.refs) {
      const list = citedBy.get(r) ?? [];
      list.push(e);
      citedBy.set(r, list);
    }
  const seqOf = new Map(events.map((e) => [e.id, e.seq]));

  for (const e of events) {
    const card = el("article", `ev k-${e.kind}`);
    card.id = `e-${e.id}`;
    const head = el("header");
    const seq = el("span", "seq", `#${e.seq}`);
    seq.id = `seq-${e.seq}`;
    head.append(
      seq,
      el("span", "kind", e.kind),
      el("span", "ts", shortTime(e.ts)),
      el("span", "from", `${e.from} (${e.from_kind})`),
    );
    card.append(head);

    const content = el("div", "content", e.content);
    card.append(content);

    const foot = el("footer");
    for (const t of e.tags) foot.append(el("span", t === "memory" ? "tag memory" : "tag", t));
    if (e.act) {
      foot.append(
        el("span", "act", e.act.tool ? `${e.act.kind}:${e.act.tool}` : e.act.kind),
      );
    }
    if (e.refs.length > 0) {
      foot.append(document.createTextNode("refs "));
      for (const r of e.refs) {
        const a = el("a", "ref", `#${seqOf.get(r) ?? "?"}`);
        a.href = `#e-${r}`;
        foot.append(a, document.createTextNode(" "));
      }
    }
    const back = citedBy.get(e.id);
    if (back && back.length > 0) {
      foot.append(document.createTextNode("← cited by "));
      for (const b of back) {
        const a = el("a", "ref", `#${b.seq}`);
        a.href = `#e-${b.id}`;
        foot.append(a, document.createTextNode(" "));
      }
    }
    if (foot.childNodes.length > 0) card.append(foot);
    root.append(card);
  }
}
