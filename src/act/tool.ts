import { timed } from "../ops.ts";
import { tagOf } from "../tool/define.ts";
import type { Tool } from "../tool/core.ts";
import type { AgentDeps, ToolCall, ToolMessage } from "../types.ts";
import type { MemoryRecord } from "../memory/index.ts";
import { admitAgentAct } from "./admit.ts";
import { effectFromTag } from "./sense.ts";
import { toolTag } from "./tag/make.ts";
import type { ActRecord } from "./types.ts";

type ParseResult =
  | { ok: true; value: Record<string, unknown> }
  | { ok: false; error: string };

const parseArgs = (raw: string): ParseResult => {
  try {
    const value: unknown = JSON.parse(raw);
    if (value !== null && typeof value === "object" && !Array.isArray(value)) {
      return { ok: true, value: value as Record<string, unknown> };
    }
    return { ok: false, error: "tool arguments must be a JSON object" };
  } catch {
    return { ok: false, error: "tool arguments are not valid JSON" };
  }
};

const envelope = (toolName: string, tag: ReturnType<typeof tagOf>): ActRecord => ({
  kind: "tool",
  tag,
  tool: toolName,
});

export type ToolActResult = {
  action?: MemoryRecord;
  observation?: MemoryRecord;
  message: ToolMessage;
};

export async function runToolAct(deps: AgentDeps, call: ToolCall): Promise<ToolActResult> {
  const tool: Tool | undefined = deps.tools.find((item) => item.name === call.name);
  const tag = tool ? tagOf(tool) : toolTag.unbounded();
  const act = envelope(call.name, tag);
  const intent = { ...act };
  const done = (effect: ActRecord["effect"]): ActRecord => ({ ...act, effect });

  const action = admitAgentAct(deps, {
    memKind: "action",
    content: `${call.name} ${call.arguments}`,
    act: intent,
  });

  const fail = async (content: string): Promise<ToolActResult> => {
    const observation = admitAgentAct(deps, {
      memKind: "observation",
      content,
      refs: action ? [action.id] : [],
      act: done(effectFromTag(tag)),
    });
    return {
      action,
      observation,
      message: { role: "tool", tool_call_id: call.id, content },
    };
  };

  if (!tool) return fail(`unknown tool: ${call.name}`);
  if (tag.permit === "forbidden") return fail("forbidden");

  const parsed = parseArgs(call.arguments);
  if (!parsed.ok) return fail(parsed.error);

  let content: string;
  try {
    const run = () => Promise.resolve(tool.execute(parsed.value, { signal: deps.signal }));
    content = deps.ops ? await timed(deps.ops, `tool.${call.name}`, run) : await run();
  } catch (err) {
    content = err instanceof Error ? err.message : String(err);
  }

  const observation = admitAgentAct(deps, {
    memKind: "observation",
    content,
    refs: action ? [action.id] : [],
    act: done(effectFromTag(tag)),
  });
  return {
    action,
    observation,
    message: { role: "tool", tool_call_id: call.id, content },
  };
}
