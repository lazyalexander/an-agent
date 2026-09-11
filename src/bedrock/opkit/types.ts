export type Dict<T = unknown, K extends string = string> = { readonly [key in K]: T };

export type Get<T extends object, K> = K extends keyof T ? T[K] : never;

export type MaybeArray<T> = T | T[];

export type Awaitable<T> = T | Promise<T>;

export function noop(): void {}

export function isNullable(value: unknown): value is null | undefined {
  return value === null || value === undefined;
}

export function isNonNullable<T>(value: T): value is NonNullable<T> {
  return !isNullable(value);
}

export function isPlainObject(value: unknown): value is Dict<unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

export function mapValues<T, U, K extends string>(
  object: Dict<T, K>,
  transform: (value: T, key: K) => U,
): Dict<U, K> {
  return Object.fromEntries(
    Object.entries(object).map(([key, value]) => [key, transform(value as T, key as K)]),
  ) as Dict<U, K>;
}

export function pick<T extends object, K extends keyof T>(source: T, keys: readonly K[]): Pick<T, K> {
  const result = {} as Pick<T, K>;
  for (const key of keys) {
    if (key in source) result[key] = source[key];
  }
  return result;
}

export function omit<T extends object, K extends keyof T>(source: T, keys: readonly K[]): Omit<T, K> {
  const skip = new Set<PropertyKey>(keys);
  const result = { ...source };
  for (const key of skip) delete (result as Record<PropertyKey, unknown>)[key];
  return result as Omit<T, K>;
}
