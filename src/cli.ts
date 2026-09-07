import { createInterface } from "node:readline/promises";
import { stdin, stdout } from "node:process";
import { runUntilIdle } from "./agent.ts";
import { createModelClient } from "./model.ts";
import { lastAssistantText, parseLine } from "./repl.ts";
import type { AgentState } from "./types.ts";

const main = async () => {
  const complete = createModelClient();
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
      state = {
        messages: [...state.messages, { role: "user", content: command.text }],
      };
      state = await runUntilIdle(state, { complete, tools: [] });
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
