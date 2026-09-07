import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

export function loadOrCreateId(path: string): string {
  if (existsSync(path)) return readFileSync(path, "utf8").trim();
  mkdirSync(dirname(path), { recursive: true });
  const id = crypto.randomUUID();
  writeFileSync(path, `${id}\n`, { mode: 0o600 });
  return id;
}

export function localPrincipals(rootDir: string): { agentId: string; humanId: string } {
  return {
    agentId: loadOrCreateId(join(rootDir, "agent-id")),
    humanId: loadOrCreateId(join(rootDir, "human-id")),
  };
}
