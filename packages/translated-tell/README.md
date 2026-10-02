# translated-tell

A page over one probe tape. The tape is produced by `translated_tell`: a terminal utterance is told to a silk address, an enhancer prefixes it, a host writer records the translated line, and a verbatim clip is refused.

The page ships the tape from one run. Drop another `tape.jsonl` to step through that one instead.

```sh
cargo test -p an-agent-probes --offline --test translated_tell
AN_AGENT_PROBE_DIR=packages/translated-tell/examples \
  cargo test -p an-agent-probes --offline --test translated_tell translated_tell -- --exact
cd packages/translated-tell && bun install && bun test && bun serve
```

`bun serve` listens on port 3778. Left and right arrows move one event.
