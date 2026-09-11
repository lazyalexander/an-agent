/** Duration helpers. Clock values are passed in; this module does not call Date.now(). */
export namespace Time {
  export const millisecond = 1;
  export const second = 1000;
  export const minute = second * 60;
  export const hour = minute * 60;
  export const day = hour * 24;
  export const week = day * 7;

  const numeric = String.raw`\d+(?:\.\d+)?`;
  const timeRegExp = new RegExp(
    `^${["w(?:eek(?:s)?)?", "d(?:ay(?:s)?)?", "h(?:our(?:s)?)?", "m(?:in(?:ute)?(?:s)?)?", "s(?:ec(?:ond)?(?:s)?)?"]
      .map((unit) => `(${numeric}${unit})?`)
      .join("")}$`,
  );

  /** Parse `1w2d3h4m5s` into milliseconds. Unrecognized input is 0. */
  export function parse(source: string): number {
    const capture = timeRegExp.exec(source);
    if (!capture) return 0;
    return (
      (Number.parseFloat(capture[1] ?? "") * week || 0) +
      (Number.parseFloat(capture[2] ?? "") * day || 0) +
      (Number.parseFloat(capture[3] ?? "") * hour || 0) +
      (Number.parseFloat(capture[4] ?? "") * minute || 0) +
      (Number.parseFloat(capture[5] ?? "") * second || 0)
    );
  }

  export function format(ms: number): string {
    const abs = Math.abs(ms);
    if (abs >= day - hour / 2) return `${Math.round(ms / day)}d`;
    if (abs >= hour - minute / 2) return `${Math.round(ms / hour)}h`;
    if (abs >= minute - second / 2) return `${Math.round(ms / minute)}m`;
    if (abs >= second) return `${Math.round(ms / second)}s`;
    return `${ms}ms`;
  }

  /** Format a millisecond timestamp as UTC ISO-8601. */
  export function iso(ms: number): string {
    return new Date(ms).toISOString();
  }
}
