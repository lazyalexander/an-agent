#!/usr/bin/env bun
// Process entry for `an-agent`. Today this is a line REPL; later it will start the TUI.
import { homedir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline/promises";
import { stdin, stdout } from "node:process";
import { runUntilIdle } from "./agent.ts";
import { loadEnv } from "./load-env.ts";
import { appendLog, createFileLog } from "./log.ts";
import { createModelClient } from "./model.ts";
import { lastAssistantText, parseLine } from "./repl.ts";
import { loadTools } from "./tools.ts";
import type { AgentState } from "./types.ts";

const main = async () => {
  loadEnv();
  const complete = createModelClient();
  const tools = loadTools();
  const log = createFileLog(
    process.env.AGENT_LOG_PATH ?? join(homedir(), ".an-agent", "log.jsonl"),
  );
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
      appendLog(log, { type: "user", content: command.text });
      state = {
        messages: [...state.messages, { role: "user", content: command.text }],
      };
      state = await runUntilIdle(state, { complete, tools, log });
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
