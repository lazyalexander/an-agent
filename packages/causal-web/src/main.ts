import { parseTape } from "./types";
import { validateTape } from "./validate";
import { renderChecks, renderSummary, renderThread } from "./render";

function $(id: string): HTMLElement {
  const e = document.getElementById(id);
  if (!e) throw new Error(`missing #${id}`);
  return e;
}

function load(name: string, text: string): void {
  const tape = parseTape(text);
  const checks = validateTape(tape);
  renderSummary($("summary"), name, tape.events, checks);
  renderChecks($("checks"), checks, (seq) => {
    const target = document.getElementById(`seq-${seq}`);
    target?.scrollIntoView({ behavior: "smooth", block: "center" });
  });
  renderThread($("thread"), tape.events);
}

const input = $("file") as HTMLInputElement;
input.addEventListener("change", async () => {
  const f = input.files?.[0];
  if (f) load(f.name, await f.text());
});

const drop = $("drop");
for (const t of ["dragover", "dragenter"])
  drop.addEventListener(t, (e) => {
    e.preventDefault();
    drop.classList.add("hot");
  });
for (const t of ["dragleave", "drop"])
  drop.addEventListener(t, (e) => {
    e.preventDefault();
    drop.classList.remove("hot");
  });
drop.addEventListener("drop", async (e) => {
  const f = (e as DragEvent).dataTransfer?.files?.[0];
  if (f) load(f.name, await f.text());
});

const initial = new URLSearchParams(location.search).get("tape") ?? "examples/sample.jsonl";
fetch(initial)
  .then((r) => (r.ok ? r.text() : Promise.reject(new Error("no tape: " + initial))))
  .then((t) => load(initial, t))
  .catch(() => {});
