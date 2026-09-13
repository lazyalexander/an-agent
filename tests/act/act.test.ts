import { describe, expect, test } from "bun:test";
import { effectFromTag, runToolAct, toolTag } from "../../src/act/index.ts";
import { defineTool } from "../../src/tool/index.ts";
import type { MemoryInput, MemoryRecord } from "../../src/memory/index.ts";
import type { ModelClient } from "../../src/types.ts";

const capturingMemory = () => {
  const records: MemoryRecord[] = [];
  return {
    records,
    deps: {
      agentId: "agent-1",
      session: "session-1",
      memory: {
        append: (input: MemoryInput): MemoryRecord => {
          const record: MemoryRecord = {
            v: 1,
            id: `id-${records.length + 1}`,
            seq: records.length + 1,
            ts: "",
            ...input,
          };
          records.push(record);
          return record;
        },
      },
    },
  };
};

const complete: ModelClient = async () => ({ role: "assistant", content: "" });

describe("effectFromTag", () => {
  test("maps file faces into reads, writes, or unbounded", () => {
    expect(effectFromTag(toolTag.none())).toEqual({
      reads: [],
      writes: [],
      unbounded: false,
      workplace: undefined,
      memory: "ignore",
    });
    expect(effectFromTag(toolTag.read("/src/a.ts")).reads).toEqual(["/src/a.ts"]);
    expect(effectFromTag(toolTag.write("/src/a.ts")).writes).toEqual(["/src/a.ts"]);
    expect(effectFromTag(toolTag.readWrite("/src")).reads).toEqual(["/src"]);
    expect(effectFromTag(toolTag.readWrite("/src")).writes).toEqual(["/src"]);
    expect(effectFromTag(toolTag.unbounded()).unbounded).toBe(true);
  });
});

describe("runToolAct", () => {
  test("writes action then observation with envelope and effect", async () => {
    const tape = capturingMemory();
    const echo = defineTool({
      name: "echo",
      description: "echo",
      parameters: {},
      tag: toolTag.none(),
      execute: (args) => String(args.text ?? ""),
    });
    const result = await runToolAct(
      { complete, tools: [echo], ...tape.deps },
      { id: "c1", name: "echo", arguments: JSON.stringify({ text: "z" }) },
    );
    expect(result.message.content).toBe("z");
    expect(result.observation?.refs).toEqual([result.action?.id ?? ""]);
    expect(result.action?.act?.kind).toBe("tool");
    expect(result.action?.act?.tool).toBe("echo");
    expect(result.observation?.act?.effect).toEqual({
      reads: [],
      writes: [],
      unbounded: false,
      workplace: undefined,
      memory: "ignore",
    });
  });

  test("forbidden skips execute", async () => {
    let ran = false;
    const tape = capturingMemory();
    const locked = defineTool({
      name: "echo",
      description: "echo",
      parameters: {},
      tag: toolTag.none("forbidden"),
      execute: () => {
        ran = true;
        return "no";
      },
    });
    const result = await runToolAct(
      { complete, tools: [locked], ...tape.deps },
      { id: "c1", name: "echo", arguments: "{}" },
    );
    expect(ran).toBe(false);
    expect(result.message.content).toBe("forbidden");
    expect(result.observation?.content).toBe("forbidden");
  });
});
