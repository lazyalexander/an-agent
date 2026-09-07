import type { Tool } from "../types.ts";

export const bash: Tool = {
  name: "bash",
  description: "Run a shell command. Arguments: { command: string }.",
  parameters: {
    type: "object",
    properties: {
      command: { type: "string", description: "Shell command to run" },
    },
    required: ["command"],
  },
  execute: async (args) => {
    const command = String(args.command ?? "");
    if (!command) return "command is required";
    const proc = Bun.spawn(["bash", "-lc", command], {
      stdout: "pipe",
      stderr: "pipe",
    });
    const stdout = await new Response(proc.stdout).text();
    const stderr = await new Response(proc.stderr).text();
    const code = await proc.exited;
    if (code === 0) return stdout;
    return `exit ${code}\n${stdout}${stderr}`;
  },
};
