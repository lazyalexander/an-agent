const ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

const encode = (value: bigint, length: number): string => {
  let out = "";
  let n = value;
  for (let i = 0; i < length; i += 1) {
    out = ALPHABET[Number(n % 32n)] + out;
    n /= 32n;
  }
  return out;
};

export function ulid(nowMs = Date.now()): string {
  const time = encode(BigInt(nowMs), 10);
  const rand = new Uint8Array(10);
  crypto.getRandomValues(rand);
  let n = 0n;
  for (const byte of rand) n = (n << 8n) | BigInt(byte);
  return time + encode(n, 16);
}
