# ts-tools

TS tools probe package: can a TypeScript tool sit behind the same `Tool` trait as a Rust builtin?

**Protocol (stdio, one shot):** one JSON object in on stdin, one JSON object out on stdout, process exits. Logs go to stderr, never stdout.

**Runtime:** bun (no install step, no node_modules — plain scripts).

**Honesty note:** a TS subprocess has ambient authority — the declared effect is a promise, not a wiring diagram (unlike the rhai sandbox, where the host-function whitelist mechanically is the effect boundary). Same trust tier as MCP.

**Status:** probe. The descriptor validator currently rejects a `ts` constructor — that rejection is part of the experiment's findings.
