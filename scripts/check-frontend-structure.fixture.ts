import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { DEEP_MODULE_BOUNDARIES, FORBIDDEN_UPWARD_IMPORTS } from "./check-frontend-structure.ts";

export interface FixtureEntry {
  name: string;
  dir: string;
  deps: string[];
  testSupport?: boolean;
  note: string;
}

export interface FixtureRepoOptions {
  yamlGlob?: boolean;
  memberDirs?: string[];
  manifest?: FixtureEntry[];
  omitModule?: boolean;
}

const tempDirs: string[] = [];

export function fixtureRepo(opts: FixtureRepoOptions = {}): string[] {
  const root = mkdtempSync(join(tmpdir(), "check-frontend-structure-fixture-"));
  tempDirs.push(root);
  const yamlLines = ["# 夹具", "allowBuilds:", "  esbuild: true"];
  if (opts.yamlGlob !== false) {
    yamlLines.unshift("packages:", "  # 夹具注释", "  - packages/*");
  }
  writeFileSync(join(root, "pnpm-workspace.yaml"), yamlLines.join("\n") + "\n");
  mkdirSync(join(root, "packages"), { recursive: true });
  if (!opts.omitModule) {
    for (const entry of DEEP_MODULE_BOUNDARIES) {
      const moduleAbs = join(root, ...entry.module.split("/"));
      mkdirSync(dirname(moduleAbs), { recursive: true });
      writeFileSync(moduleAbs, "export const fixtureRegisteredModule = true\n");
    }
  }
  for (const rule of FORBIDDEN_UPWARD_IMPORTS) {
    mkdirSync(join(root, ...rule.dir.split("/")), { recursive: true });
  }
  for (const dir of opts.memberDirs ?? []) {
    mkdirSync(join(root, "packages", dir), { recursive: true });
  }
  const args = [root];
  if (opts.manifest) {
    const manifestPath = join(root, "fixture-manifest.json");
    writeFileSync(manifestPath, JSON.stringify(opts.manifest, null, 2));
    args.push(manifestPath);
  }
  return args;
}

export function cleanupFixtureRepos(): void {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true });
  tempDirs.length = 0;
}
