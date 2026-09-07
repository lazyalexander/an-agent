# an-agent

Minimal terminal agent. Core loop is a function. Model and tools are injected.

## Run

```bash
bun install
export DEEPSEEK_API_KEY=...
bun start
```

Type a line, get a reply. `/exit` quits.

## Test

```bash
bun test
bun run typecheck
```

Loop tests never call the network. Live DeepSeek tests skip without `DEEPSEEK_API_KEY`.
