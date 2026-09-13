export type ToolContext = {
  /** Aborts when the surrounding turn is cancelled (e.g. user interrupt). */
  signal?: AbortSignal;
};

/**
 * Model-facing tool. Workplace faces live on the act envelope, not here.
 * defineTool may attach an optional tag seed for wrapping.
 */
export type Tool = {
  name: string;
  description: string;
  parameters: Record<string, unknown>;
  execute: (args: Record<string, unknown>, ctx?: ToolContext) => Promise<string> | string;
};
