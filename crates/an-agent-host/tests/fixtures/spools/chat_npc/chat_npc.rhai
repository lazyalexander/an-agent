// One beat of a discord character: answer one message in persona.
// event.body is "author: text"; config.persona names the mask.
// One body, many mounts — the persona is mount config, not code.
// (rhai's trim() returns unit; the delimiter carries the space.)
let parts = event.body.split(": ");
let author = parts[0];
let said = parts[1];
config.persona + " @" + author + " heard «" + said + "»"
