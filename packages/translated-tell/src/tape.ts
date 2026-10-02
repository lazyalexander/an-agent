export interface ActOnEvent {
  kind: string;
  tag?: { form?: string; permit?: string };
  tool?: string;
}

export interface Memevent {
  v: number;
  id: string;
  seq: number;
  ts: string;
  from: string;
  from_kind: string;
  kind: string;
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

export function jsonContent(event: Memevent): Record<string, unknown> | null {
  try {
    const value = JSON.parse(event.content) as unknown;
    if (value && typeof value === "object" && !Array.isArray(value)) {
      return value as Record<string, unknown>;
    }
  } catch {
    return null;
  }
  return null;
}
