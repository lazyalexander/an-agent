#!/usr/bin/env bun
// Process entry for `an-agent`. Today this is a line REPL; later it will start the TUI.
import { createInterface } from "node:readline/promises";
import { stdin, stdout } from "node:process";
import { runUntilIdle } from "./agent.ts";
import { loadEnv } from "./load-env.ts";
import { createModelClient } from "./model.ts";
import { lastAssistantText, readCommand } from "./repl.ts";
import type { AgentState } from "./types.ts";

const main = async () => {
  loadEnv();
  const complete = createModelClient();
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
      state = {
        messages: [...state.messages, { role: "user", content: command.text }],
      };
      try {
        state = await runUntilIdle(state, { complete, tools: [] });
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
