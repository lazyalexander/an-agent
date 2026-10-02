import { FIXTURE } from "./fixture";
import { claims, frame } from "./story";
import { parseTape, type Memevent } from "./tape";

const tapeEl = document.querySelector("#tape") as HTMLOListElement;
const claimsEl = document.querySelector("#claims") as HTMLUListElement;
const stateEl = document.querySelector("#state") as HTMLElement;
const captionEl = document.querySelector("#caption") as HTMLElement;
const posEl = document.querySelector("#pos") as HTMLElement;
const dropEl = document.querySelector("#drop") as HTMLElement;

let events: Memevent[] = parseTape(FIXTURE).events;
let index = 0;
let timer: ReturnType<typeof setInterval> | undefined;

function render(): void {
  const prefix = events.slice(0, index + 1);
  const now = frame(prefix);
  captionEl.textContent = now.caption;
  posEl.textContent = events.length === 0 ? "0 / 0" : `${index + 1} / ${events.length}`;

  tapeEl.replaceChildren(
    ...events.map((event, i) => {
      const li = document.createElement("li");
      const button = document.createElement("button");
      button.type = "button";
      button.className = i === index ? "on" : "";
      const who = event.act?.tool ?? event.from;
      button.textContent = `${i + 1}. ${who}`;
      button.addEventListener("click", () => {
        stop();
        index = i;
        render();
      });
      li.append(button);
      return li;
    }),
  );
  const current = tapeEl.children[index];
  current?.scrollIntoView({ block: "nearest" });

  claimsEl.replaceChildren(
    ...claims.map((claim) => {
      const li = document.createElement("li");
      li.className = claim.holds(prefix) ? "on" : "";
      li.textContent = claim.title;
      return li;
    }),
  );

  const mounted = now.mounted.length ? now.mounted.join("、") : "（空）";
  const written = now.editor.length ? now.editor.join("、") : "（还没有）";
  stateEl.replaceChildren();
  for (const [label, value] of [
    ["挂着", mounted],
    ["editor 写下", written],
    ["信封载荷", now.payload ?? "（还没有）"],
  ] as const) {
    const row = document.createElement("p");
    const name = document.createElement("span");
    name.textContent = label;
    row.append(name, document.createTextNode(value));
    stateEl.append(row);
  }
}

function stop(): void {
  if (timer) clearInterval(timer);
  timer = undefined;
}

function go(next: number): void {
  if (events.length === 0) return;
  index = Math.max(0, Math.min(events.length - 1, next));
  render();
}

document.querySelector("#prev")?.addEventListener("click", () => {
  stop();
  go(index - 1);
});
document.querySelector("#next")?.addEventListener("click", () => {
  stop();
  go(index + 1);
});
document.querySelector("#play")?.addEventListener("click", () => {
  if (timer) {
    stop();
    return;
  }
  timer = setInterval(() => {
    if (index >= events.length - 1) {
      stop();
      return;
    }
    go(index + 1);
  }, 700);
});

document.addEventListener("keydown", (event) => {
  if (event.key === "ArrowLeft") go(index - 1);
  if (event.key === "ArrowRight") go(index + 1);
});

dropEl.addEventListener("dragover", (event) => {
  event.preventDefault();
  dropEl.classList.add("hot");
});
dropEl.addEventListener("dragleave", () => dropEl.classList.remove("hot"));
dropEl.addEventListener("drop", (event) => {
  event.preventDefault();
  dropEl.classList.remove("hot");
  const file = event.dataTransfer?.files[0];
  if (!file) return;
  void file.text().then((text) => {
    const parsed = parseTape(text);
    if (parsed.events.length === 0) return;
    stop();
    events = parsed.events;
    index = 0;
    render();
  });
});

render();
