// word_count: a probe TS tool. Protocol: one JSON object in on stdin,
// one JSON object out on stdout. Nothing else on stdout (log to stderr).
//   in:  { "text": "..." }
//   out: { "words": 3, "chars": 11 }

const input = (await Bun.stdin.json()) as { text?: unknown };
const text = typeof input.text === "string" ? input.text : "";
const trimmed = text.trim();
const out = {
  words: trimmed === "" ? 0 : trimmed.split(/\s+/).length,
  chars: text.length,
};
process.stdout.write(JSON.stringify(out) + "\n");
