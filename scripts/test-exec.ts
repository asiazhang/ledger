#!/usr/bin/env bun

import { spawn } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { cpus } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import {
  hasExtension,
  lineAt,
  maskNonCode,
  RUST_EXTENSIONS,
  walkTextFiles,
} from "./gate-primitives.ts";

const SRC_TAURI_DIR_NAME = "src-tauri";

const TEST_SH_REL = join("scripts", "test.sh");

export const PARALLEL_ENTRY_MARKER = "scripts/test-exec.ts";

export const DELEGATED_TARGET_FLAG = "--test";
export const DOC_FLAG = "--doc";

export const WORKSPACE_FLAG = "--workspace";
export const ALL_FLAG = "--all";

export type TargetKind = "lib" | "bin" | "test" | "doc";

export interface DiscoveredTarget {
  package: string;
  target: string;
  kind: TargetKind;
  harnessFalse: boolean;
  cwd: string;
}

export function targetKey(t: { package: string; target: string }): string {
  return `${t.package}::${t.target}`;
}

export function runKey(t: { package: string; target: string; kind: TargetKind }): string {
  return `${t.package}::${t.target}::${t.kind}`;
}

export interface ExemptBinDecl {
  package: string;
  target: string;
  owner: string;
}

export const EXEMPT_BINS: ExemptBinDecl[] = [
  {
    package: "tauri-app",
    target: "ledger-perf",
    owner: "perf-bench.yml 每日基准专属入口（--bin ledger-perf 显式选择）",
  },
];

export interface TestFalseBin {
  package: string;
  target: string;
  source: string | null;
}

interface Discovery {
  targets: DiscoveredTarget[];
  problems: string[];
  outOfFace: string[];
  testFalseBins: TestFalseBin[];
}

interface TargetDecl {
  name?: string;
  path?: string;
  test?: boolean;
  harness?: boolean;
  doctest?: boolean;
}

interface ManifestTables {
  package: { name?: string };
  lib: TargetDecl | null;
  bins: TargetDecl[];
  tests: TargetDecl[];
  benches: TargetDecl[];
  examples: TargetDecl[];
  workspace: { members?: string[] };
  unsupportedKeys: string[];
}

const ARRAY_DECL_TABLES: Record<string, "bins" | "tests" | "benches" | "examples"> = {
  bin: "bins",
  test: "tests",
  bench: "benches",
  example: "examples",
};

const UNSUPPORTED_KEYS = ["autobins", "autotests", "autobenches", "autoexamples"];

function stripComment(line: string): string {
  let quote: string | null = null;
  for (let i = 0; i < line.length; i += 1) {
    const ch = line[i];
    if (quote !== null) {
      if (ch === quote) quote = null;
      continue;
    }
    if (ch === '"' || ch === "'") {
      quote = ch;
      continue;
    }
    if (ch === "#") return line.slice(0, i);
  }
  return line;
}

function bracketBalance(line: string): number {
  let depth = 0;
  let quote: string | null = null;
  for (const ch of line) {
    if (quote !== null) {
      if (ch === quote) quote = null;
      continue;
    }
    if (ch === '"' || ch === "'") quote = ch;
    else if (ch === "[") depth += 1;
    else if (ch === "]") depth -= 1;
  }
  return depth;
}

function parseValue(raw: string): unknown {
  const text = raw.trim();
  const quoted = text.match(/^(["'])(.*)\1$/);
  if (quoted !== null) return quoted[2];
  if (text === "true") return true;
  if (text === "false") return false;
  if (text.startsWith("[") && text.endsWith("]")) {
    return splitArrayItems(text.slice(1, -1)).map((item) => parseValue(item));
  }
  return text;
}

function splitArrayItems(body: string): string[] {
  const items: string[] = [];
  let current = "";
  let quote: string | null = null;
  for (const ch of body) {
    if (quote !== null) {
      current += ch;
      if (ch === quote) quote = null;
      continue;
    }
    if (ch === '"' || ch === "'") {
      quote = ch;
      current += ch;
      continue;
    }
    if (ch === ",") {
      items.push(current);
      current = "";
      continue;
    }
    current += ch;
  }
  items.push(current);
  return items.map((i) => i.trim()).filter((i) => i !== "");
}

function parseManifest(text: string): ManifestTables {
  const out: ManifestTables = {
    package: {},
    lib: null,
    bins: [],
    tests: [],
    benches: [],
    examples: [],
    workspace: {},
    unsupportedKeys: [],
  };
  let table: string | null = null;
  let decl: TargetDecl | null = null;
  const lines = text.split("\n");
  for (let i = 0; i < lines.length; i += 1) {
    let line = stripComment(lines[i] ?? "").trim();
    if (line === "") continue;
    while (bracketBalance(line) > 0 && i + 1 < lines.length) {
      i += 1;
      line += ` ${stripComment(lines[i] ?? "").trim()}`;
    }

    const arrayHeader = line.match(/^\[\[\s*([A-Za-z0-9_-]+)\s*\]\]$/);
    if (arrayHeader !== null) {
      const key = ARRAY_DECL_TABLES[arrayHeader[1] ?? ""];
      if (key === undefined) {
        table = null;
        decl = null;
      } else {
        decl = {};
        table = key;
        out[key].push(decl);
      }
      continue;
    }
    const tableHeader = line.match(/^\[\s*([A-Za-z0-9_-]+)\s*\]$/);
    if (tableHeader !== null) {
      const name = tableHeader[1] ?? "";
      decl = null;
      table = name;
      if (name === "lib") out.lib = {};
      continue;
    }
    if (line.startsWith("[")) {
      decl = null;
      table = null;
      continue;
    }
    const kv = line.match(/^([A-Za-z0-9_-]+)\s*=\s*(.+)$/);
    if (kv === null) continue;
    const key = kv[1] ?? "";
    const value = parseValue(kv[2] ?? "");
    if (UNSUPPORTED_KEYS.includes(key)) out.unsupportedKeys.push(key);
    const target = decl ?? (table === "lib" ? out.lib : null);
    if (target !== null) {
      applyTargetKey(target, key, value);
      continue;
    }
    if (table === "package" && key === "name" && typeof value === "string")
      out.package.name = value;
    if (table === "workspace" && key === "members" && Array.isArray(value)) {
      out.workspace.members = value.filter((v): v is string => typeof v === "string");
    }
  }
  return out;
}

function applyTargetKey(target: TargetDecl, key: string, value: unknown): void {
  if (key === "name" && typeof value === "string") target.name = value;
  else if (key === "path" && typeof value === "string") target.path = value;
  else if (key === "test" && typeof value === "boolean") target.test = value;
  else if (key === "harness" && typeof value === "boolean") target.harness = value;
  else if (key === "doctest" && typeof value === "boolean") target.doctest = value;
}

function readManifest(dir: string): ManifestTables | null {
  const path = join(dir, "Cargo.toml");
  if (!existsSync(path)) return null;
  return parseManifest(readFileSync(path, "utf8"));
}

function listDir(dir: string): string[] {
  if (!existsSync(dir)) return [];
  return readdirSync(dir).sort();
}

function isDirectory(path: string): boolean {
  return existsSync(path) && statSync(path).isDirectory();
}

function stemName(file: string): string {
  return file.replace(/\.rs$/, "");
}

function expandMemberGlobs(
  srcTauriDir: string,
  patterns: string[],
): { dirs: string[]; problems: string[] } {
  const dirs: string[] = [];
  const problems: string[] = [];
  for (const pattern of patterns) {
    if (pattern.endsWith("/*")) {
      const base = pattern.slice(0, -2);
      const baseDir = join(srcTauriDir, base);
      if (!isDirectory(baseDir)) {
        problems.push(`✗ 目标发现：members glob 基目录不存在：${base}（${pattern}）`);
        continue;
      }
      for (const entry of listDir(baseDir)) {
        const entryDir = join(baseDir, entry);
        if (isDirectory(entryDir) && existsSync(join(entryDir, "Cargo.toml"))) dirs.push(entryDir);
      }
      continue;
    }
    if (pattern.includes("*")) {
      problems.push(
        `✗ 目标发现：未支持的 members glob 形态：${pattern}（只支持「前缀 + 尾随 *」单层 glob）`,
      );
      continue;
    }
    const dir = join(srcTauriDir, pattern);
    if (!existsSync(join(dir, "Cargo.toml"))) {
      problems.push(`✗ 目标发现：members 目录缺 Cargo.toml：${pattern}`);
      continue;
    }
    dirs.push(dir);
  }
  return { dirs, problems };
}

interface PackageTargets {
  targets: DiscoveredTarget[];
  problems: string[];
  outOfFace: string[];
  testFalseBins: TestFalseBin[];
}

function resolveBinSource(
  dir: string,
  packageName: string,
  decl: TargetDecl,
  name: string,
): string | null {
  if (decl.path !== undefined)
    return existsSync(join(dir, decl.path)) ? join(dir, decl.path) : null;
  const candidates: string[] = [];
  if (name === packageName) candidates.push(join("src", "main.rs"));
  candidates.push(join("src", "bin", `${name}.rs`), join("src", "bin", name, "main.rs"));
  for (const rel of candidates) {
    if (existsSync(join(dir, rel))) return join(dir, rel);
  }
  return null;
}

function discoverPackage(dir: string, rel: string, tables: ManifestTables): PackageTargets {
  const targets: DiscoveredTarget[] = [];
  const problems: string[] = [];
  const outOfFace: string[] = [];
  const testFalseBins: TestFalseBin[] = [];
  const packageName = tables.package.name;
  if (packageName === undefined) {
    problems.push(`✗ 目标发现：${rel}/Cargo.toml 缺 [package] name——目标无法归属包，拒绝继续`);
    return { targets, problems, outOfFace, testFalseBins };
  }
  for (const key of tables.unsupportedKeys) {
    problems.push(
      `✗ 目标发现：${rel}/Cargo.toml 出现 auto* 开关 \`${key}\`——自动发现面被改写，` +
        `发现逻辑不辖该形态（拒绝猜测）。请扩展 scripts/test-exec.ts 的目标发现后重跑`,
    );
  }

  const libDeclared = tables.lib !== null;
  const libPresent = libDeclared || existsSync(join(dir, "src", "lib.rs"));
  if (libPresent) {
    const libName = tables.lib?.name ?? packageName.replace(/-/g, "_");
    if (tables.lib?.harness === false) {
      problems.push(
        `✗ 目标发现：${rel} 的 [lib] 声明 harness = false——自定义 harness 的 lib 单测` +
          `不属 libtest 面，需显式归入非并发入口（当前发现逻辑不支持）`,
      );
    } else if (tables.lib?.test !== false) {
      targets.push({
        package: packageName,
        target: libName,
        kind: "lib",
        harnessFalse: false,
        cwd: dir,
      });
    }
    if (tables.lib?.doctest !== false) {
      targets.push({
        package: packageName,
        target: libName,
        kind: "doc",
        harnessFalse: false,
        cwd: dir,
      });
    }
  }

  const binNames = new Map<string, TargetDecl>();
  for (const decl of tables.bins) binNames.set(decl.name ?? stemName(decl.path ?? ""), decl);
  for (const candidate of [{ name: packageName, path: join("src", "main.rs") }].concat(
    listDir(join(dir, "src", "bin")).flatMap((entry) => {
      const entryPath = join(dir, "src", "bin", entry);
      if (hasExtension(entry, RUST_EXTENSIONS))
        return [{ name: stemName(entry), path: join("src", "bin", entry) }];
      if (isDirectory(entryPath) && existsSync(join(entryPath, "main.rs"))) {
        return [{ name: entry, path: join("src", "bin", entry, "main.rs") }];
      }
      return [];
    }),
  )) {
    if (!existsSync(join(dir, candidate.path))) continue;
    if (!binNames.has(candidate.name))
      binNames.set(candidate.name, { name: candidate.name, path: candidate.path });
  }
  for (const [name, decl] of binNames) {
    if (decl.harness === false) {
      problems.push(
        `✗ 目标发现：${rel} 的 [[bin]] ${name} 声明 harness = false——自定义 harness 不属并发面，需显式归入非并发入口`,
      );
      continue;
    }
    if (decl.test === false) {
      testFalseBins.push({
        package: packageName,
        target: name,
        source: resolveBinSource(dir, packageName, decl, name),
      });
      continue;
    }
    targets.push({
      package: packageName,
      target: name,
      kind: "bin",
      harnessFalse: false,
      cwd: dir,
    });
  }

  const testDecls = new Map<string, TargetDecl>();
  for (const decl of tables.tests) testDecls.set(decl.name ?? stemName(decl.path ?? ""), decl);
  for (const candidate of listDir(join(dir, "tests")).flatMap((entry) => {
    const entryPath = join(dir, "tests", entry);
    if (hasExtension(entry, RUST_EXTENSIONS)) return [{ name: stemName(entry) }];
    if (isDirectory(entryPath) && existsSync(join(entryPath, "main.rs"))) return [{ name: entry }];
    return [];
  })) {
    if (!testDecls.has(candidate.name)) testDecls.set(candidate.name, { name: candidate.name });
  }
  for (const [name, decl] of testDecls) {
    if (decl.test === false) continue;
    targets.push({
      package: packageName,
      target: name,
      kind: "test",
      harnessFalse: decl.harness === false,
      cwd: dir,
    });
  }

  for (const key of ["benches", "examples"] as const) {
    for (const decl of tables[key]) {
      const name = decl.name ?? stemName(decl.path ?? "");
      if (name !== "") outOfFace.push(`${rel}/${key === "benches" ? "bench" : "example"}:${name}`);
    }
    for (const entry of listDir(join(dir, key))) {
      if (hasExtension(entry, RUST_EXTENSIONS))
        outOfFace.push(`${rel}/${key === "benches" ? "bench" : "example"}:${stemName(entry)}`);
    }
  }

  return { targets, problems, outOfFace, testFalseBins };
}

export function discoverTargets(rootDir: string): Discovery {
  const srcTauriDir = join(rootDir, SRC_TAURI_DIR_NAME);
  const problems: string[] = [];
  const targets: DiscoveredTarget[] = [];
  const outOfFace: string[] = [];
  const testFalseBins: TestFalseBin[] = [];

  const rootTables = readManifest(srcTauriDir);
  if (rootTables === null) {
    return {
      targets,
      problems: [`✗ 目标发现：Rust 根 manifest 不存在：${SRC_TAURI_DIR_NAME}/Cargo.toml`],
      outOfFace,
      testFalseBins,
    };
  }
  const packageDirs: { dir: string; rel: string }[] = [];
  if (rootTables.package.name !== undefined) {
    packageDirs.push({ dir: srcTauriDir, rel: SRC_TAURI_DIR_NAME });
  }
  const memberPatterns = rootTables.workspace.members;
  if (memberPatterns === undefined) {
    if (packageDirs.length === 0) {
      problems.push(
        `✗ 目标发现：${SRC_TAURI_DIR_NAME}/Cargo.toml 既无 [package] 也无 [workspace] members`,
      );
    }
  } else {
    const expanded = expandMemberGlobs(srcTauriDir, memberPatterns);
    problems.push(...expanded.problems);
    for (const dir of expanded.dirs) {
      packageDirs.push({ dir, rel: relative(rootDir, dir) });
    }
  }

  const seen = new Set<string>();
  for (const { dir, rel } of packageDirs) {
    const tables = readManifest(dir);
    if (tables === null) {
      problems.push(`✗ 目标发现：成员目录缺 Cargo.toml：${rel}`);
      continue;
    }
    const found = discoverPackage(dir, rel, tables);
    problems.push(...found.problems);
    outOfFace.push(...found.outOfFace);
    testFalseBins.push(...found.testFalseBins);
    for (const target of found.targets) {
      const key = runKey(target);
      if (seen.has(key)) continue;
      seen.add(key);
      targets.push(target);
    }
  }
  return { targets, problems, outOfFace, testFalseBins };
}

const SHELL_CONTROL_CHARS = new Set(["&", "|", ";", "(", ")"]);

const COMMAND_PREFIX_WORDS = new Set([
  "command",
  "exec",
  "env",
  "nohup",
  "time",
  "if",
  "then",
  "else",
  "elif",
  "do",
  "while",
  "until",
  "!",
]);

const ENV_ASSIGNMENT_PREFIX = /^[A-Za-z_][A-Za-z0-9_]*=/;

const COMMAND_LINE_DECORATIONS = new Set(["-", "run:"]);

function tokenizeShellLine(line: string): string[] {
  const tokens: string[] = [];
  let current = "";
  let started = false;
  const flush = (): void => {
    if (started) tokens.push(current);
    current = "";
    started = false;
  };
  for (let i = 0; i < line.length; i += 1) {
    const c = line[i] as string;
    if (c === "'" || c === '"') {
      let j = i + 1;
      while (j < line.length) {
        const inner = line[j] as string;
        if (c === '"' && inner === "\\") {
          current += line.slice(j, j + 2);
          j += 2;
          continue;
        }
        if (inner === c) break;
        current += inner;
        j += 1;
      }
      started = true;
      i = j;
      continue;
    }
    if (c === " " || c === "\t") {
      flush();
      continue;
    }
    if (SHELL_CONTROL_CHARS.has(c)) {
      flush();
      tokens.push(c);
      continue;
    }
    current += c;
    started = true;
  }
  flush();
  return tokens;
}

function stripCommandPrefixes(tokens: string[]): string[] {
  for (let start = 0; start < tokens.length; start += 1) {
    const token = tokens[start] as string;
    if (
      COMMAND_LINE_DECORATIONS.has(token) ||
      COMMAND_PREFIX_WORDS.has(token) ||
      ENV_ASSIGNMENT_PREFIX.test(token)
    ) {
      continue;
    }
    return tokens.slice(start);
  }
  return [];
}

function shellCommands(content: string): string[][] {
  const commands: string[][] = [];
  for (const rawLine of content.split("\n")) {
    const line = rawLine.trim();
    if (line === "" || line.startsWith("#")) continue;
    let segment: string[] = [];
    const flush = (): void => {
      const argv = stripCommandPrefixes(segment);
      if (argv.length > 0) commands.push(argv);
      segment = [];
    };
    for (const token of tokenizeShellLine(line)) {
      if (SHELL_CONTROL_CHARS.has(token)) flush();
      else segment.push(token);
    }
    flush();
  }
  return commands;
}

export interface EntryWiring {
  tests: Set<string>;
  doc: boolean;
  docWorkspace: boolean;
  parallel: boolean;
}

export function parseEntryWiring(content: string): EntryWiring {
  const tests = new Set<string>();
  let doc = false;
  let docWorkspace = false;
  let parallel = false;
  for (const argv of shellCommands(content)) {
    if (argv[0] === "bun") {
      const marker = argv.indexOf(PARALLEL_ENTRY_MARKER, 1);
      if (marker !== -1) {
        const subcommand = argv.slice(marker + 1).find((t) => !t.startsWith("-"));
        if (subcommand === undefined || subcommand === "run") parallel = true;
      }
      continue;
    }
    if (argv[0] !== "cargo" || argv[1] !== "test") continue;
    const args = argv.slice(2);
    if (args.includes(DOC_FLAG)) {
      doc = true;
      if (args.includes(WORKSPACE_FLAG) || args.includes(ALL_FLAG)) docWorkspace = true;
    }
    for (let i = 0; i < args.length; i += 1) {
      if (args[i] === DELEGATED_TARGET_FLAG) {
        const name = args[i + 1] ?? "";
        if (name !== "") tests.add(name);
      }
    }
  }
  return { tests, doc, docWorkspace, parallel };
}

export interface CoverageResult {
  problems: string[];
  targets: DiscoveredTarget[];
  parallel: DiscoveredTarget[];
  delegated: DiscoveredTarget[];
  doc: DiscoveredTarget[];
  exempt: DiscoveredTarget[];
  outOfFace: string[];
}

const CARGO_RUNTIME_ENV_PATTERN = /\benv::var(?:_os)?\s*\(\s*"(CARGO_[A-Z0-9_]+|OUT_DIR)"/g;

function cargoRuntimeEnvProblems(rootDir: string): string[] {
  const problems: string[] = [];
  const scanRoot = join(rootDir, SRC_TAURI_DIR_NAME);
  if (!isDirectory(scanRoot)) return problems;
  const files = walkTextFiles(scanRoot, "", {
    extensions: RUST_EXTENSIONS,
    skipDirs: new Set(["target"]),
  }).filter((f) => f.rel.split("/").pop() !== "build.rs");
  for (const file of files) {
    const masked = maskNonCode(readFileSync(file.abs, "utf8"), true);
    for (const match of masked.matchAll(CARGO_RUNTIME_ENV_PATTERN)) {
      const line = lineAt(masked, match.index);
      problems.push(
        `✗ 执行器等价性：${relative(rootDir, file.abs)}:${line} 运行期读 cargo 注入环境变量 \`${match[1]}\`——` +
          `执行器只对齐 cwd 与 RUST_TEST_THREADS，不复制该环境，测试将读到未定义值。` +
          `请改走 cwd 相对定位，或先扩展 scripts/test-exec.ts 的 runChild 并更新本守门（issue #1112）`,
      );
    }
  }
  return problems;
}

const TEST_ATTR_PATTERN = /#\[\s*(?:tokio\s*::\s*)?test\b|#\[\s*cfg\s*\(\s*test\b/g;

function testFalseBinProblems(rootDir: string, bins: TestFalseBin[]): string[] {
  const problems: string[] = [];
  for (const bin of bins) {
    if (bin.source === null) {
      problems.push(
        `✗ bin test=false 守门：${bin.package}::${bin.target} 声明 test = false，但无法定位源文件` +
          `（显式 [[bin]] 缺 path 时按 cargo 默认 src/main.rs 或 src/bin/<name>.rs 解析失败）——拒绝猜测`,
      );
      continue;
    }
    const masked = maskNonCode(readFileSync(bin.source, "utf8"), true);
    for (const match of masked.matchAll(TEST_ATTR_PATTERN)) {
      const line = lineAt(masked, match.index);
      problems.push(
        `✗ bin test=false 守门：${relative(rootDir, bin.source)}:${line} 出现测试属性 \`${match[0]}\`——` +
          `该 bin 已声明 test = false（不构建测试二进制），新增测试会静默漏跑。` +
          `请把测试移到 lib 或集成测试，或改回 test = true 并重新评估执行面（issue #1787）`,
      );
    }
  }
  return problems;
}

export function checkCoverage(rootDir: string): CoverageResult {
  const discovery = discoverTargets(rootDir);
  const problems = [...discovery.problems];
  const delegated = discovery.targets.filter((t) => t.kind === "test" && t.harnessFalse);
  const doc = discovery.targets.filter((t) => t.kind === "doc");
  const exempt = discovery.targets.filter(
    (t) =>
      t.kind === "bin" && EXEMPT_BINS.some((e) => e.package === t.package && e.target === t.target),
  );
  for (const t of discovery.targets) {
    if (t.kind !== "bin") continue;
    if (EXEMPT_BINS.some((e) => e.package === t.package && e.target === t.target)) continue;
    problems.push(
      `✗ 覆盖守门：bin 单测未登记豁免：${targetKey(t)}::bin（test = true）会进入并发入口——` +
        `bin 单测不入 PR 执行面（issue #1787 热点 2）：零单测请在 [[bin]] 声明 test = false，` +
        `有单测且有专属承接入口请在 scripts/test-exec.ts 的 EXEMPT_BINS 登记`,
    );
  }
  const exemptKeys = new Set(exempt.map(runKey));
  const parallel = discovery.targets.filter(
    (t) => t.kind !== "doc" && !(t.kind === "test" && t.harnessFalse) && !exemptKeys.has(runKey(t)),
  );

  if (discovery.targets.length === 0 && problems.length === 0) {
    problems.push("✗ 覆盖守门：工作区测试目标清单为空——拒绝以空集假绿通过");
  }

  const wiring = parseEntryWiring(readFileSync(join(rootDir, TEST_SH_REL), "utf8"));
  for (const target of delegated) {
    if (!wiring.tests.has(target.target)) {
      problems.push(
        `✗ 覆盖守门：${targetKey(target)} 声明 harness = false（自定义 runner），` +
          `但 ${TEST_SH_REL} 的非并发入口没有 \`${DELEGATED_TARGET_FLAG} ${target.target}\`——` +
          `两个入口的覆盖范围都不含它，新增自定义 harness 目标被静默漏跑`,
      );
    }
  }
  const delegatedNames = new Set(delegated.map((t) => t.target));
  for (const name of wiring.tests) {
    if (!delegatedNames.has(name)) {
      problems.push(
        `✗ 覆盖守门：${TEST_SH_REL} 的 \`${DELEGATED_TARGET_FLAG} ${name}\` 不对应任何` +
          ` harness = false 目标（登记失效或拼写漂移：非并发入口的每一项都必须是自定义 harness 目标）`,
      );
    }
  }
  if (doc.length > 0 && !wiring.doc) {
    problems.push(
      `✗ 覆盖守门：工作区有 ${doc.length} 个 doc-test 目标，但 ${TEST_SH_REL} 缺` +
        ` \`cargo test --workspace ${DOC_FLAG}\`——doc-test 静默漏跑`,
    );
  }
  if (doc.length > 0 && wiring.doc && !wiring.docWorkspace) {
    problems.push(
      `✗ 覆盖守门：${TEST_SH_REL} 的 \`${DOC_FLAG}\` 缺 workspace 范围` +
        `（\`${WORKSPACE_FLAG}\` 或 \`${ALL_FLAG}\`）——非虚拟 workspace 下只跑根包的` +
        ` doctest，其余成员 crate 的 doc-test 静默漏跑（守门仍绿）`,
    );
  }
  if (doc.length === 0 && wiring.doc) {
    problems.push(
      `✗ 覆盖守门：${TEST_SH_REL} 有 \`${DOC_FLAG}\`，但工作区无 doc-test 目标（登记失效）`,
    );
  }
  if (!wiring.parallel) {
    problems.push(
      `✗ 覆盖守门：${TEST_SH_REL} 未调用并发入口（${PARALLEL_ENTRY_MARKER}）——` +
        `单元测试与集成测试未由统一执行器调度（删除接线即红）`,
    );
  }
  problems.push(...cargoRuntimeEnvProblems(rootDir));
  problems.push(...testFalseBinProblems(rootDir, discovery.testFalseBins));

  return {
    problems,
    targets: discovery.targets,
    parallel,
    delegated,
    doc,
    exempt,
    outOfFace: discovery.outOfFace,
  };
}

function summarizeCoverage(result: CoverageResult): string {
  const count = (kind: TargetKind): number => result.parallel.filter((t) => t.kind === kind).length;
  const docPkgs = new Set(result.doc.map((t) => t.package)).size;
  const exemptOwners = (t: DiscoveredTarget): string =>
    EXEMPT_BINS.find((e) => e.package === t.package && e.target === t.target)?.owner ?? "?";
  return (
    `✓ 测试执行覆盖守门：目标 ${result.targets.length} 个 = ` +
    `并发入口 ${result.parallel.length}（lib 单测 ${count("lib")} + bin 单测 ${count("bin")} + 集成测试 ${count("test")}）` +
    ` ⊎ 非并发入口 ${result.delegated.length + result.doc.length}` +
    `（cargo 自有 runner：${result.delegated.map((t) => t.target).join(" / ") || "（无）"}` +
    ` + doc-test ${result.doc.length} 个，覆盖 ${docPkgs} 个包）` +
    (result.exempt.length > 0
      ? ` ⊎ bin 单测豁免 ${result.exempt.length} 个（${result.exempt.map((t) => `${t.target} → ${exemptOwners(t)}`).join(" / ")}）`
      : "") +
    (result.outOfFace.length > 0
      ? ` · 默认执行面外 ${result.outOfFace.length} 个（example/bench，不跑）`
      : "")
  );
}

interface ChildResult {
  code: number;
  stdout: string;
  stderr: string;
}

function runChild(
  command: string,
  args: string[],
  opts: { cwd: string; env?: NodeJS.ProcessEnv; streamStderr?: boolean },
): Promise<ChildResult> {
  return new Promise((resolvePromise) => {
    const child = spawn(command, args, { cwd: opts.cwd, env: opts.env ?? process.env });
    let stdout = "";
    let stderr = "";
    child.stdout?.setEncoding("utf8");
    child.stderr?.setEncoding("utf8");
    child.stdout?.on("data", (chunk: string) => {
      stdout += chunk;
    });
    child.stderr?.on("data", (chunk: string) => {
      stderr += chunk;
      if (opts.streamStderr === true) process.stderr.write(chunk);
    });
    child.on("error", (error) => {
      stderr += `${String(error)}\n`;
    });
    child.on("close", (code) => {
      resolvePromise({ code: code ?? -1, stdout, stderr });
    });
  });
}

interface CargoMessage {
  reason?: string;
  package_id?: string;
  target?: { name?: string; kind?: string[] };
  profile?: { test?: boolean };
  executable?: string | null;
  success?: boolean;
}

function parseCargoMessage(line: string): CargoMessage | null {
  if (!line.startsWith("{")) return null;
  try {
    return JSON.parse(line) as CargoMessage;
  } catch {
    return null;
  }
}

async function buildTestBinaries(
  cargo: string,
  srcTauriDir: string,
): Promise<{ executables: Map<string, string>; problems: string[]; ms: number }> {
  const start = Date.now();
  const result = await runChild(
    cargo,
    [
      "test",
      "--workspace",
      "--lib",
      "--test",
      "*",
      "--no-run",
      "--message-format=json-render-diagnostics",
    ],
    { cwd: srcTauriDir, streamStderr: true },
  );
  const ms = Date.now() - start;
  const problems: string[] = [];
  const executables = new Map<string, string>();
  let buildSucceeded = result.code === 0;
  for (const line of result.stdout.split("\n")) {
    const message = parseCargoMessage(line);
    if (message === null) continue;
    if (message.reason === "build-finished") buildSucceeded = message.success === true;
    if (message.reason !== "compiler-artifact") continue;
    if (message.profile?.test !== true) continue;
    const executable = message.executable;
    if (executable === null || executable === undefined) continue;
    const packageName = packageNameFromId(message.package_id ?? "");
    const targetName = message.target?.name ?? "";
    if (packageName === "" || targetName === "") continue;
    const kind = artifactTargetKind(message.target?.kind ?? []);
    if (kind === null) continue;
    executables.set(`${packageName}::${targetName}::${kind}`, executable);
  }
  if (!buildSucceeded) {
    problems.push(
      "✗ 构建失败：`cargo test --workspace --lib --test '*' --no-run` 非零退出（编译错误见上方 cargo 输出）",
    );
  }
  return { executables, problems, ms };
}

function packageNameFromId(id: string): string {
  const hash = id.indexOf("#");
  const tail = hash === -1 ? id : id.slice(hash + 1);
  const at = tail.indexOf("@");
  return at === -1 ? tail : tail.slice(0, at);
}

function artifactTargetKind(kinds: string[]): TargetKind | null {
  if (kinds.includes("test")) return "test";
  if (kinds.includes("bin")) return "bin";
  if (kinds.includes("lib") || kinds.includes("rlib") || kinds.includes("cdylib")) return "lib";
  if (kinds.includes("staticlib") || kinds.includes("dylib") || kinds.includes("proc-macro"))
    return "lib";
  return null;
}

async function runPool<T>(
  items: T[],
  jobs: number,
  worker: (item: T) => Promise<void>,
): Promise<void> {
  let cursor = 0;
  const width = Math.max(1, Math.min(jobs, items.length));
  await Promise.all(
    Array.from({ length: width }, async () => {
      for (;;) {
        const index = cursor;
        cursor += 1;
        if (index >= items.length) return;
        await worker(items[index] as T);
      }
    }),
  );
}

interface TargetOutcome {
  key: string;
  ms: number;
  code: number;
  output: string;
  passed: number;
  failed: number;
  ignored: number;
}

const TEST_RESULT_RE =
  /^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; \d+ measured; \d+ filtered out; finished in ([\d.]+)s$/gm;

function parseTestResult(output: string): { passed: number; failed: number; ignored: number } {
  let passed = 0;
  let failed = 0;
  let ignored = 0;
  for (const match of output.matchAll(TEST_RESULT_RE)) {
    passed = Number(match[1]);
    failed = Number(match[2]);
    ignored = Number(match[3]);
  }
  return { passed, failed, ignored };
}

function formatSeconds(ms: number): string {
  return `${(ms / 1000).toFixed(2)}s`;
}

const FRESH_RELINK_HINT_MS = 3_000;

interface RunOptions {
  rootDir: string;
  jobs: number;
}

async function runAll(options: RunOptions): Promise<number> {
  const { rootDir, jobs } = options;
  const coverage = checkCoverage(rootDir);
  if (coverage.problems.length > 0) printProblems(coverage.problems, "❌ 测试执行覆盖守门失败");
  else console.log(summarizeCoverage(coverage));

  const srcTauriDir = join(rootDir, SRC_TAURI_DIR_NAME);
  const cargo = process.env.CARGO ?? "cargo";
  console.log(
    `▶ 构建测试二进制（一次构建全部 target）：cargo test --workspace --lib --test '*' --no-run` +
      `（CI PR 口径同款，bin 单测不入面；缓存命中时秒级返回；冷构建的编译耗时见 cargo 输出）`,
  );
  const build = await buildTestBinaries(cargo, srcTauriDir);
  console.log(`  · 构建阶段 ${formatSeconds(build.ms)}`);
  if (build.ms > FRESH_RELINK_HINT_MS) {
    console.log(
      `  · 提示：构建阶段超过 3s = 测试二进制被重新链接——macOS 对新链接的二进制` +
        `首次执行有一次性安全扫描/页入开销（大体量二进制可达秒级），下方 per-binary` +
        ` 耗时含该一次性成本，不代表测试或夹具的真实耗时（issue #1787 的误诊源头）`,
    );
  }
  const problems = [...coverage.problems, ...build.problems];

  const exemptKeys = new Set(coverage.exempt.map(runKey));
  const expected = new Map(
    coverage.targets
      .filter((t) => t.kind !== "doc" && !exemptKeys.has(runKey(t)))
      .map((t) => [runKey(t), t] as const),
  );
  for (const key of expected.keys()) {
    if (!build.executables.has(key)) {
      problems.push(
        `✗ 执行面漂移：目标清单里的 ${key} 未被 cargo 构建（cargo test --workspace --lib --test '*' --no-run 无对应可执行文件）`,
      );
    }
  }
  for (const key of build.executables.keys()) {
    if (!expected.has(key)) {
      problems.push(
        `✗ 执行面漂移：cargo 构建出清单外的测试二进制 ${key}——目标发现逻辑漏项，` +
          `该目标会被静默漏跑（请扩展 scripts/test-exec.ts 的目标发现）`,
      );
    }
  }
  if (problems.length > 0) {
    printProblems(problems, "❌ 测试执行前检查失败");
  }

  const delegatedKeys = new Set(coverage.delegated.map((t) => runKey(t)));
  const queue = [...expected.keys()].filter((key) => !delegatedKeys.has(key)).sort();
  const cpuCount = cpus().length;
  const width = Math.max(1, Math.min(jobs, queue.length));
  console.log(
    `▶ 并发执行 ${queue.length} 个测试二进制（全局并行度 ${width} = ` +
      `min(并行度上限 ${jobs}（默认 CPU 数 ${cpuCount}）, 待跑二进制数 ${queue.length})；` +
      `每个二进制 RUST_TEST_THREADS=1（只约束 libtest 线程），libtest 线程数 = ${width}）`,
  );
  if (jobs > cpuCount) {
    console.log(
      `  · 提示：--jobs ${jobs} 超过 CPU 数 ${cpuCount}——这是显式超订（默认取 CPU 数），` +
        `刻意压测/限流时才这样传`,
    );
  }
  if (coverage.delegated.length > 0) {
    console.log(
      `  · 非并发入口承接 ${coverage.delegated.length} 个（${coverage.delegated.map((t) => t.target).join(" / ")}）+ ` +
        `doc-test ${coverage.doc.length} 个，由 ${TEST_SH_REL} 的 cargo 自有入口执行`,
    );
  }

  const start = Date.now();
  const outcomes: TargetOutcome[] = [];
  await runPool(queue, jobs, async (key) => {
    const target = expected.get(key);
    const executable = build.executables.get(key);
    if (target === undefined || executable === undefined) return;
    const itemStart = Date.now();
    const result = await runChild(executable, [], {
      cwd: target.cwd,
      env: { ...process.env, RUST_TEST_THREADS: "1" },
    });
    const ms = Date.now() - itemStart;
    const counts = parseTestResult(`${result.stdout}\n${result.stderr}`);
    outcomes.push({
      key,
      ms,
      code: result.code,
      output: `${result.stdout}${result.stderr}`,
      ...counts,
    });
    const mark = result.code === 0 ? "✓" : "✗";
    const detail = result.code === 0 ? `${counts.passed} passed` : `exit ${result.code}`;
    console.log(
      `  ${mark} ${key} ${formatSeconds(ms)}（${detail}${counts.ignored > 0 ? `, ${counts.ignored} ignored` : ""}）`,
    );
  });
  const wallMs = Date.now() - start;

  const failed = outcomes.filter((o) => o.code !== 0).sort((a, b) => a.key.localeCompare(b.key));
  if (failed.length > 0) {
    for (const outcome of failed) {
      console.error(
        `\n──── ✗ ${outcome.key}（exit ${outcome.code}，${formatSeconds(outcome.ms)}）────`,
      );
      console.error(outcome.output.trimEnd());
    }
  }

  const sumMs = outcomes.reduce((acc, o) => acc + o.ms, 0);
  const slowest = outcomes.reduce<TargetOutcome | null>(
    (acc, o) => (acc === null || o.ms > acc.ms ? o : acc),
    null,
  );
  const totals = outcomes.reduce(
    (acc, o) => ({
      passed: acc.passed + o.passed,
      failed: acc.failed + o.failed,
      ignored: acc.ignored + o.ignored,
    }),
    { passed: 0, failed: 0, ignored: 0 },
  );
  const passCount = outcomes.length - failed.length;
  console.log(
    `\n${failed.length === 0 ? "✅" : "❌"} 并发入口结果：${passCount}/${outcomes.length} 个二进制通过` +
      `（${totals.passed} passed / ${totals.failed} failed / ${totals.ignored} ignored）`,
  );
  console.log(
    `  · 墙钟 ${formatSeconds(wallMs)}（并行度 ${width}）；各二进制耗时之和 ${formatSeconds(sumMs)}；` +
      `串行等价 ≈ ${formatSeconds(sumMs + build.ms)}；加速 ${(sumMs / Math.max(wallMs, 1)).toFixed(2)}x` +
      (slowest !== null ? `；最长 ${slowest.key} ${formatSeconds(slowest.ms)}` : ""),
  );
  return failed.length === 0 ? 0 : 1;
}

function printProblems(problems: string[], title: string): never {
  for (const problem of problems) console.error(problem);
  console.error(`${title}：${problems.length} 处问题（issue #1112）`);
  process.exit(1);
}

interface CliOptions {
  command: "run" | "check" | "plan";
  rootDir: string;
  jobs: number;
}

function parseArgs(argv: string[], defaultRoot: string): CliOptions {
  let command: CliOptions["command"] = "run";
  let rootDir = defaultRoot;
  let jobs = Number(process.env.LEDGER_TEST_JOBS ?? "") || cpus().length;
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "run" || arg === "check" || arg === "plan") command = arg;
    else if (arg === "--jobs") {
      i += 1;
      const value = Number(argv[i] ?? "");
      if (!Number.isInteger(value) || value < 1) {
        console.error(`✗ --jobs 需要一个正整数，收到：${argv[i] ?? "（缺参数）"}`);
        process.exit(2);
      }
      jobs = value;
    } else if (arg === "--root") {
      i += 1;
      rootDir = resolve(argv[i] ?? "");
    } else {
      console.error(
        `✗ 未知参数：${arg}（用法：bun scripts/test-exec.ts [run|check|plan] [--jobs N] [--root DIR]）`,
      );
      process.exit(2);
    }
  }
  return { command, rootDir, jobs };
}

async function main(): Promise<void> {
  const defaultRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
  const options = parseArgs(process.argv.slice(2), defaultRoot);
  if (options.command === "plan") {
    const discovery = discoverTargets(options.rootDir);
    if (discovery.problems.length > 0) printProblems(discovery.problems, "❌ 目标发现失败");
    const exemptKeys = new Set(
      discovery.targets
        .filter(
          (t) =>
            t.kind === "bin" &&
            EXEMPT_BINS.some((e) => e.package === t.package && e.target === t.target),
        )
        .map(runKey),
    );
    for (const target of discovery.targets) {
      const note = target.harnessFalse
        ? "  harness=false（非并发入口）"
        : exemptKeys.has(runKey(target))
          ? "  bin 单测豁免（EXEMPT_BINS，不入并发入口）"
          : "";
      console.log(`${target.kind.padEnd(5)} ${targetKey(target)}${note}`);
    }
    return;
  }
  if (options.command === "check") {
    const coverage = checkCoverage(options.rootDir);
    if (coverage.problems.length > 0) printProblems(coverage.problems, "❌ 测试执行覆盖守门失败");
    console.log(summarizeCoverage(coverage));
    return;
  }
  process.exitCode = await runAll(options);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
