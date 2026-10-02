import { jsonContent, type Memevent } from "./tape";

export interface Claim {
  id: string;
  title: string;
  holds: (events: Memevent[]) => boolean;
}

function permit(event: Memevent): string | undefined {
  return event.act?.tag?.permit;
}

function isMount(event: Memevent, name: string, how: "go" | "deny"): boolean {
  return (
    event.act?.kind === "mount" &&
    event.act.tool === name &&
    permit(event) === how
  );
}

function firstIndex(events: Memevent[], pred: (event: Memevent) => boolean): number {
  return events.findIndex(pred);
}

export const claims: Claim[] = [
  {
    id: "rights",
    title: "权利不够的挂载被拒绝，成功挂载都在这两次拒绝之后",
    holds: (events) => {
      const clerk = firstIndex(events, (e) => isMount(e, "clerk", "deny"));
      const scribe = firstIndex(events, (e) => isMount(e, "scribe", "deny"));
      const go = firstIndex(
        events,
        (e) => e.act?.kind === "mount" && permit(e) === "go",
      );
      return clerk >= 0 && scribe > clerk && (go < 0 || go > scribe);
    },
  },
  {
    id: "mounted",
    title: "translator、editor、box、scribe、clerk、naive 都挂上了",
    holds: (events) =>
      ["translator", "editor", "box", "scribe", "clerk", "naive"].every((name) =>
        events.some((e) => isMount(e, name, "go")),
      ),
  },
  {
    id: "address",
    title: "发往不存在的地址被拒绝，此前没有信封",
    holds: (events) => {
      const denied = firstIndex(
        events,
        (e) =>
          e.tags.includes("silk") &&
          e.tags.includes("deny") &&
          e.content.includes("no such silk address"),
      );
      if (denied < 0) return false;
      return !events.slice(0, denied).some((e) => e.tags.includes("tell"));
    },
  },
  {
    id: "translated",
    title: "信封载荷是译:hello，并引用了翻译那一跳",
    holds: (events) =>
      events.some((event) => {
        if (!event.tags.includes("tell") || event.refs.length !== 1) return false;
        const body = jsonContent(event);
        const payload = body?.payload as { text?: string } | undefined;
        if (payload?.text !== "译:hello") return false;
        const hop = events.find((e) => e.id === event.refs[0]);
        return hop?.tags.includes("enhance") === true;
      }),
  },
  {
    id: "heard",
    title: "box 说出的预览里带有译:hello",
    holds: (events) =>
      events.some(
        (e) =>
          e.kind === "utterance" &&
          e.from === "box" &&
          e.content.includes("译:hello"),
      ),
  },
  {
    id: "recorded",
    title: "editor 只执行了一次，参数是 scribe 自己写下的译:hello",
    holds: (events) =>
      events.filter(
        (e) =>
          e.kind === "action" &&
          e.act?.tool === "editor" &&
          e.content.includes("译:hello"),
      ).length === 1,
  },
  {
    id: "taint",
    title: "naive 把人类原文送进 editor 的尝试被污点门挡住",
    holds: (events) => events.some((e) => e.tags.includes("taint")),
  },
  {
    id: "unmount",
    title: "translator 卸下记 none，editor 卸下记 irreversible",
    holds: (events) => {
      const note = (name: string) =>
        events.find(
          (e) => e.act?.kind === "unmount" && e.act.tool === name,
        );
      const translator = note("translator");
      const editor = note("editor");
      return (
        translator?.content.includes('"inverse":"none"') === true &&
        editor?.content.includes('"inverse":"irreversible"') === true
      );
    },
  },
];

export interface Frame {
  mounted: string[];
  editor: string[];
  payload: string | null;
  caption: string;
}

function editorLine(event: Memevent): string | null {
  if (event.kind !== "action" || event.act?.tool !== "editor") return null;
  const raw = event.content.replace(/^editor\s+/, "");
  try {
    const args = JSON.parse(raw) as { content?: unknown };
    return typeof args.content === "string" ? args.content : null;
  } catch {
    return null;
  }
}

export function caption(event: Memevent): string {
  if (event.act?.kind === "mount" && permit(event) === "deny") {
    return `${event.act.tool} 的闭包超出了这次权利，挂载被拒绝`;
  }
  if (event.act?.kind === "mount" && permit(event) === "go") {
    return `${event.act.tool} 挂上了`;
  }
  if (event.act?.kind === "unmount") {
    const body = jsonContent(event);
    return `${event.act.tool} 卸下，逆是 ${String(body?.inverse ?? "")}`;
  }
  if (event.tags.includes("link")) return "translator 接到 box 的入口";
  if (event.kind === "utterance" && event.from_kind === "human") {
    return `终端说：${event.content}`;
  }
  if (event.tags.includes("deny") && event.tags.includes("silk")) {
    return "这个地址没有挂上，信封没有发出";
  }
  if (event.tags.includes("enhance")) return "translator 改写了载荷";
  if (event.tags.includes("tell")) return "信封落到 box，载荷已经是译文";
  if (event.tags.includes("taint")) return "人类原文不能进入会写文件的 editor";
  if (event.kind === "utterance" && event.from === "box") return "box 把预览说了出来";
  const line = editorLine(event);
  if (line) return `editor 写下「${line}」`;
  if (event.kind === "observation") return `${event.from} 的决定`;
  if (event.kind === "action" && event.act?.kind === "invoke") {
    return `${event.from} 走了一步`;
  }
  return event.kind;
}

export function frame(events: Memevent[]): Frame {
  const mounted: string[] = [];
  const editor: string[] = [];
  let payload: string | null = null;
  for (const event of events) {
    if (event.act?.kind === "mount" && permit(event) === "go" && event.act.tool) {
      if (!mounted.includes(event.act.tool)) mounted.push(event.act.tool);
    }
    if (event.act?.kind === "unmount" && event.act.tool) {
      const at = mounted.indexOf(event.act.tool);
      if (at >= 0) mounted.splice(at, 1);
    }
    const line = editorLine(event);
    if (line) editor.push(line);
    if (event.tags.includes("tell")) {
      const body = jsonContent(event);
      const inner = body?.payload as { text?: string } | undefined;
      if (typeof inner?.text === "string") payload = inner.text;
    }
  }
  const current = events[events.length - 1];
  return {
    mounted,
    editor,
    payload,
    caption: current ? caption(current) : "磁带还没开始",
  };
}
