import { spawnSync } from "node:child_process";
import { join } from "node:path";

export interface RunResult {
  status: number;
  output: string;
}

export function gateScript(name: string): string {
  return join(process.cwd(), "scripts", name);
}

export function runGateScript(
  script: string,
  args: readonly string[] = [],
  opts: { cwd?: string } = {},
): RunResult {
  const r = spawnSync("bun", [script, ...args], { encoding: "utf8", cwd: opts.cwd });
  return { status: r.status ?? -1, output: (r.stdout ?? "") + (r.stderr ?? "") };
}
