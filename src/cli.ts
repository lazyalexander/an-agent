import { createInterface } from "node:readline/promises";
import { stdin, stdout } from "node:process";
import { runUntilIdle } from "./agent.ts";
import { createDeepSeekClient } from "./deepseek.ts";
import type { AgentState } from "./types.ts";

const main = async () => {
  const complete = createDeepSeekClient();
  let state: AgentState = {
    messages: [{ role: "system", content: "You are a helpful assistant." }],
  };
  const rl = createInterface({ input: stdin, output: stdout });
  stdout.write("an-agent. /exit to quit.\n");

  try {
    for (;;) {
      const line = (await rl.question("> ")).trim();
      if (line === "" ) continue;
      if (line === "/exit") break;
      state = {
        messages: [...state.messages, { role: "user", content: line }],
      };
      state = await runUntilIdle(state, { complete, tools: [] });
      const last = state.messages.at(-1);
      const text = last && last.role === "assistant" ? last.content : "";
      stdout.write(`${text}\n`);
    }
  } finally {
    rl.close();
  }
};

main().catch((err) => {
  console.error(err instanceof Error ? err.message : err);
  process.exitCode = 1;
});
