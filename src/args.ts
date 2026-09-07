export type CliArgs = {
  debugOps?: string;
};

export function parseCliArgs(argv: readonly string[]): CliArgs {
  const out: CliArgs = {};
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--debug-ops") {
      const path = argv[i + 1];
      if (!path || path.startsWith("-")) throw new Error("--debug-ops requires a path");
      out.debugOps = path;
      i += 1;
    }
  }
  return out;
}
