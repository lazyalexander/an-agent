#!/usr/bin/env bun
// Process entry for `an-agent`. Today this is a line REPL; later it will start the TUI.
import { homedir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline/promises";
import { stdin, stdout } from "node:process";
import { runUntilIdle } from "./agent.ts";
import { loadEnv } from "./load-env.ts";
import { createJsonlMemoryStore } from "./memory.ts";
import { createModelClient } from "./model.ts";
import { localPrincipals } from "./principals.ts";
import { lastAssistantText, parseLine } from "./repl.ts";
import { loadTools } from "./tools.ts";
import type { AgentState } from "./types.ts";

const main = async () => {
  loadEnv();
  const complete = createModelClient();
  const tools = loadTools();
  const root = join(homedir(), ".an-agent");
  const { agentId, humanId } = localPrincipals(root);
  const memory = createJsonlMemoryStore(join(root, "agents", agentId, "memory.jsonl"));
  let state: AgentState = {
    messages: [{ role: "system", content: "You are a helpful assistant." }],
  };
  const rl = createInterface({ input: stdin, output: stdout });
  stdout.write("an-agent. /exit to quit.\n");

  try {
    for (;;) {
      const command = parseLine(await rl.question("> "));
      if (command.kind === "skip") continue;
      if (command.kind === "exit") break;
      memory.append({
        from: humanId,
        from_kind: "human",
        kind: "utterance",
        tags: ["utterance"],
        content: command.text,
        refs: [],
      });
      state = {
        messages: [...state.messages, { role: "user", content: command.text }],
      };
      state = await runUntilIdle(state, { complete, tools, agentId, memory });
      stdout.write(`${lastAssistantText(state)}\n`);
    }
  } finally {
    rl.close();
  }
};

main().catch((err) => {
  console.error(err instanceof Error ? err.message : err);
  process.exitCode = 1;
});
