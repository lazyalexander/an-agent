export function capitalize(source: string): string {
  return source.charAt(0).toUpperCase() + source.slice(1);
}

export function uncapitalize(source: string): string {
  return source.charAt(0).toLowerCase() + source.slice(1);
}

export function camelCase(source: string): string {
  return source.replace(/[_-][a-z]/g, (chunk) => chunk.slice(1).toUpperCase());
}

const tokenize = (source: string, delimiter: string): string => {
  return source
    .replace(/[A-Z]+/g, (chunk, offset) =>
      (offset > 0 ? delimiter : "") + chunk.toLowerCase(),
    )
    .replace(/[_-]+/g, delimiter)
    .replace(new RegExp(`${delimiter}{2,}`, "g"), delimiter)
    .replace(new RegExp(`^${delimiter}|${delimiter}$`, "g"), "");
};

export function paramCase(source: string): string {
  return tokenize(source, "-");
}

export function snakeCase(source: string): string {
  return tokenize(source, "_");
}

export const camelize = camelCase;
export const hyphenate = paramCase;
