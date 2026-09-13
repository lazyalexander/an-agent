import type { FromKind, Kind, MemoryRecord } from "../memory/index.ts";
import type { AgentDeps } from "../types.ts";
import type { ActRecord } from "./types.ts";
import { toolTag } from "./tag/make.ts";

export type AdmitDeps = Pick<AgentDeps, "memory" | "agentId" | "session">;

export type AdmitActInput = {
  memKind: Kind;
  content: string;
  refs?: readonly string[];
  from: string;
  from_kind: FromKind;
  act?: ActRecord;
};

export function admitAct(deps: AdmitDeps, input: AdmitActInput): MemoryRecord | undefined {
  if (!deps.memory || !deps.agentId || !deps.session) return undefined;
  return deps.memory.append({
    from: input.from,
    from_kind: input.from_kind,
    kind: input.memKind,
    session: deps.session,
    tags: [],
    content: input.content,
    refs: input.refs ?? [],
    ...(input.act ? { act: input.act } : {}),
  });
}

export function admitAgentAct(
  deps: AdmitDeps,
  input: Omit<AdmitActInput, "from" | "from_kind">,
): MemoryRecord | undefined {
  if (!deps.agentId) return undefined;
  return admitAct(deps, {
    ...input,
    from: deps.agentId,
    from_kind: "agent",
  });
}

export function admitUtterance(deps: AdmitDeps, content: string): MemoryRecord | undefined {
  return admitAgentAct(deps, {
    memKind: "utterance",
    content,
    act: { kind: "utterance", tag: toolTag.none() },
  });
}
