import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export type EnvMap = Record<string, string | undefined>;

export const applyEnvFile = (path: string, env: EnvMap): void => {
  if (!existsSync(path)) return;
  const text = readFileSync(path, "utf8");
  for (const raw of text.split("\n")) {
    const line = raw.trim();
    if (line === "" || line.startsWith("#")) continue;
    const stripped = line.startsWith("export ") ? line.slice(7).trim() : line;
    const eq = stripped.indexOf("=");
    if (eq <= 0) continue;
    const key = stripped.slice(0, eq).trim();
    let value = stripped.slice(eq + 1).trim();
    if (
      (value.startsWith("\"") && value.endsWith("\"")) ||
      (value.startsWith("'") && value.endsWith("'"))
    ) {
      value = value.slice(1, -1);
    }
    if (env[key] === undefined || env[key] === "") env[key] = value;
  }
};

export const packageRoot = (fromUrl = import.meta.url): string =>
  join(dirname(fileURLToPath(fromUrl)), "..");

export const loadEnv = (env: EnvMap = process.env): void => {
  applyEnvFile(join(homedir(), ".an-agent", ".env"), env);
  applyEnvFile(join(packageRoot(), ".env"), env);
};
