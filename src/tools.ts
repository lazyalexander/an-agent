import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { packageRoot } from "./load-env.ts";
import { bash } from "./tools/bash.ts";
import type { Tool } from "./types.ts";

const registry: Record<string, Tool> = { bash };

type ToolsFile = { enabled?: string[] };

export function loadTools(configPath = join(packageRoot(), "config", "tools.json")): Tool[] {
  if (!existsSync(configPath)) return [];
  const file = JSON.parse(readFileSync(configPath, "utf8")) as ToolsFile;
  return (file.enabled ?? []).map((name) => {
    const tool = registry[name];
    if (!tool) throw new Error(`unknown tool in config: ${name}`);
    return tool;
  });
}
