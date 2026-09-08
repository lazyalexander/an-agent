#!/usr/bin/env bun
// Process entry for `an-agent`. Today this is a line REPL; later it will start the TUI.
import { homedir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline/promises";
import { stdin, stdout } from "node:process";
import { runUntilIdle } from "./agent.ts";
import { parseCliArgs } from "./args.ts";
import { loadEnv } from "./load-env.ts";
import { createJsonlMemoryStore } from "./memory/index.ts";
import { createModelClient } from "./model.ts";
import { resolveOps, timed } from "./ops.ts";
import { localAgentId, stdinCounterpartId } from "./principals.ts";
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
  const agentId = localAgentId(root);
  const stdinFrom = stdinCounterpartId(agentId);
  const memory = createJsonlMemoryStore(join(root, "agents", agentId, "memory.jsonl"));
  // Temporary session: new UUID per process. Stdin has no protocol tag, so
  // from_kind is unknown; from is derived from this agent, not a shared human id.
  const session = crypto.randomUUID();
  let state: AgentState = {
    messages: [{ role: "system", content: "You are a helpful assistant." }],
  };
  const rl = createInterface({ input: stdin, output: stdout });
  stdout.write("an-agent. /exit to quit. Ctrl+C cancels the current turn.\n");

  // Ctrl+C: at the prompt quits like /exit; during a turn cancels that turn
  // (tool processes and the in-flight model request get aborted); a second
  // Ctrl+C mid-cancel force-exits.
  let turn: AbortController | undefined;
  rl.on("SIGINT", () => {
    if (!turn) {
      rl.close();
      return;
    }
    if (turn.signal.aborted) process.exit(130);
    turn.abort();
  });

  try {
    for (;;) {
      const command = await readCommand(() => rl.question("> "));
      if (command.kind === "skip") continue;
      if (command.kind === "exit") break;
      memory.append({
        from: stdinFrom,
        from_kind: "unknown",
        kind: "utterance",
        session,
        tags: [],
        content: command.text,
        refs: [],
      });
      state = {
        messages: [...state.messages, { role: "user", content: command.text }],
      };
      const ctrl = new AbortController();
      turn = ctrl;
      try {
        state = await timed(ops, "agent.turn", () =>
          runUntilIdle(state, { complete, tools, agentId, session, memory, ops, signal: ctrl.signal }),
        );
        if (ctrl.signal.aborted) {
          stdout.write("(turn cancelled)\n");
        } else {
          stdout.write(`${lastAssistantText(state)}\n`);
        }
      } catch (err) {
        console.error(err instanceof Error ? err.message : err);
      } finally {
        turn = undefined;
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
