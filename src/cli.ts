#!/usr/bin/env bun
// Process entry for `an-agent`. Today this is a line REPL; later it will start the TUI.
import { homedir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline/promises";
import { stdin, stdout } from "node:process";
import { runUntilIdle } from "./agent.ts";
import { parseCliArgs } from "./args.ts";
import { loadEnv } from "./load-env.ts";
import { createJsonlMemoryStore } from "./memory.ts";
import { createModelClient } from "./model.ts";
import { resolveOps, timed } from "./ops.ts";
import { localPrincipals } from "./principals.ts";
import { lastAssistantText, readCommand } from "./repl.ts";
import { loadTools } from "./tools.ts";
import type { AgentState } from "./types.ts";

const main = async () => {
  loadEnv();
  const args = parseCliArgs(process.argv.slice(2));
  const ops = resolveOps({ debugPath: args.debugOps, env: process.env });
  const complete = createModelClient({ ops });
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
      const command = await readCommand(() => rl.question("> "));
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
      try {
        state = await timed(ops, "agent.turn", () =>
          runUntilIdle(state, { complete, tools, agentId, memory, ops }),
        );
        stdout.write(`${lastAssistantText(state)}\n`);
      } catch (err) {
        console.error(err instanceof Error ? err.message : err);
      }
    }
  } finally {
    rl.close();
  }
};

main().catch((err) => {
  console.error(err instanceof Error ? err.message : err);
  process.exitCode = 1;
});
