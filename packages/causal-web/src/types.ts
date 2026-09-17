export type FromKind = "human" | "agent" | "unknown";
export type Kind = "utterance" | "action" | "observation";

export interface ActOnEvent {
  kind: string;
  tag?: unknown;
  tool?: string;
  effect?: unknown;
}

export interface Memevent {
  v: number;
  id: string;
  seq: number;
  ts: string;
  from: string;
  from_kind: FromKind;
  kind: Kind;
  session?: string;
  content: string;
  tags: string[];
  refs: string[];
  act?: ActOnEvent;
}

export interface ParsedTape {
  events: Memevent[];
  parseErrors: { line: number; message: string }[];
}

export function parseTape(text: string): ParsedTape {
  const events: Memevent[] = [];
  const parseErrors: { line: number; message: string }[] = [];
  text.split("\n").forEach((raw, i) => {
    const line = raw.trim();
    if (!line) return;
    try {
      events.push(JSON.parse(line) as Memevent);
    } catch (e) {
      parseErrors.push({ line: i + 1, message: String(e) });
    }
  });
  return { events, parseErrors };
}
