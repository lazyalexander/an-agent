import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

const DNS_NAMESPACE = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";

const uuidToBytes = (id: string): Uint8Array => {
  const hex = id.replaceAll("-", "");
  const out = new Uint8Array(16);
  for (let i = 0; i < 16; i += 1) {
    out[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
};

const bytesToUuid = (bytes: Uint8Array): string => {
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20, 32)}`;
};

export function uuidv5(namespace: string, name: string): string {
  const hash = createHash("sha1").update(uuidToBytes(namespace)).update(name, "utf8").digest();
  hash[6] = (hash[6] & 0x0f) | 0x50;
  hash[8] = (hash[8] & 0x3f) | 0x80;
  return bytesToUuid(hash.subarray(0, 16));
}

/** Namespace for unauthenticated stdin counterparts. Derived, then frozen in meaning. */
const STDIN_NAMESPACE = uuidv5(DNS_NAMESPACE, "an-agent.stdin");

export function loadOrCreateId(path: string): string {
  if (existsSync(path)) return readFileSync(path, "utf8").trim();
  mkdirSync(dirname(path), { recursive: true });
  const id = crypto.randomUUID();
  writeFileSync(path, `${id}\n`, { mode: 0o600 });
  return id;
}

export function localAgentId(rootDir: string): string {
  return loadOrCreateId(join(rootDir, "agent-id"));
}

/** Principal for this agent's stdin channel. Not a machine-global human. */
export function stdinCounterpartId(agentId: string): string {
  return uuidv5(STDIN_NAMESPACE, `${agentId}/stdin`);
}
