export type ToolContext = {
  /** Aborts when the surrounding turn is cancelled (e.g. user interrupt). */
  signal?: AbortSignal;
};

/**
 * Model-facing tool. Extra workplace faces live on `tag` via defineTool,
 * not on this type.
 */
export type Tool = {
  name: string;
  description: string;
  parameters: Record<string, unknown>;
  execute: (args: Record<string, unknown>, ctx?: ToolContext) => Promise<string> | string;
};
