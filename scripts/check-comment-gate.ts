#!/usr/bin/env bun

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import {
  hasExtension,
  maskComments,
  maskNonCode,
  readDirEntries,
  walkTextFiles,
} from "./gate-primitives.ts";

const DEFAULT_ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const BASELINE_REL_SEGMENTS = ["scripts", "fixtures", "comment-baseline.json"];
const CORPUS_DIR_PREFIX = "scripts/fixtures/";
const SKIP_DIRS = new Set(["node_modules", "target", "dist", ".git"]);
const WALK_EXTENSIONS = new Set([".rs", ".ts", ".vue", ".js", ".sh", ".toml", ".json"]);
const RUST_EXTENSIONS = new Set([".rs"]);
const SCRIPT_EXTENSIONS = new Set([".ts", ".vue", ".js"]);
const SHELL_EXTENSIONS = new Set([".sh"]);
const TOML_BASENAMES = new Set(["Cargo.toml", ".kache.toml"]);
const SAFETY_FORM = /^\/\/[!/]?\s*SAFETY[:：（(]/;
const COMPILE_FAIL_FENCE_OPEN = /^\/\/[!/]\s*(`{3,}|~{3,}).*compile_fail/;
const FENCE_CLOSE = /^\/\/[!/]\s*(`{3,}|~{3,})\s*$/;
const SHEBANG = /^#!/;
const LINE_LEADING_HASH = /^\s*#/;
const PACKAGE_JSON_COMMENT_KEY = /^\s*"\/\/"\s*:/;
const FILE_KEYS = [
  ".kache.toml",
  "package.json",
  "resume-dev.sh",
  "vite.config.ts",
  "vitest.config.ts",
  "vitest.bench.config.ts",
];
const DIR_KEYS = ["src", "scripts", "src-tauri", "src-tauri/src", "src-tauri/tests"];

type Face = "rust" | "script" | "shell" | "toml" | "packageJson";

interface CommentLine {
  line: number;
  text: string;
}

interface Counted {
  count: number;
  lines: CommentLine[];
  safetyExempt: number;
  shebangExempt: number;
  compileFailExempt: number;
}

interface FileCount {
  rel: string;
  count: number;
  lines: CommentLine[];
}

interface KeyStats {
  actual: number;
  files: FileCount[];
}

interface ScanResult {
  keys: string[];
  stats: Map<string, KeyStats>;
  unmatched: string[];
  total: number;
  safetyExempt: number;
  shebangExempt: number;
  compileFailExempt: number;
  fileCount: number;
}

interface Baseline {
  fatal?: string;
  missing: string[];
  extra: string[];
  invalid: [string, unknown][];
  values: Map<string, number>;
}

function fail(message: string): never {
  console.error(`✗ 注释禁令棘轮守门：${message}`);
  process.exit(1);
}

function faceOf(name: string): Face | undefined {
  if (hasExtension(name, RUST_EXTENSIONS)) return "rust";
  if (hasExtension(name, SCRIPT_EXTENSIONS)) return "script";
  if (hasExtension(name, SHELL_EXTENSIONS)) return "shell";
  if (TOML_BASENAMES.has(name)) return "toml";
  if (name === "package.json") return "packageJson";
  return undefined;
}

function deriveKeys(root: string): string[] {
  const keys: string[] = [];
  for (const key of FILE_KEYS) {
    if (existsSync(join(root, key))) keys.push(key);
  }
  for (const key of DIR_KEYS) {
    if (existsSync(join(root, key))) keys.push(key);
  }
  const cratesDir = join(root, "src-tauri", "crates");
  if (existsSync(cratesDir)) {
    for (const entry of readDirEntries(cratesDir)) {
      if (entry.isDirectory) keys.push(`src-tauri/crates/${entry.name}`);
    }
  }
  const packagesDir = join(root, "packages");
  if (existsSync(packagesDir)) {
    for (const entry of readDirEntries(packagesDir)) {
      if (entry.isDirectory) keys.push(`packages/${entry.name}`);
    }
  }
  return keys.sort((a, b) => a.localeCompare(b));
}

function matchKey(rel: string, keys: readonly string[]): string | undefined {
  let matched: string | undefined;
  for (const key of keys) {
    if (rel !== key && !rel.startsWith(`${key}/`)) continue;
    if (matched === undefined || key.length > matched.length) matched = key;
  }
  return matched;
}

function isMaskedCommentLine(src: string, msk: string): boolean {
  const width = Math.max(src.length, msk.length);
  for (let col = 0; col < width; col++) {
    if (msk[col] !== src[col]) return true;
  }
  return false;
}

function compileFailFenceExemptLines(
  sourceLines: readonly string[],
  maskedLines: readonly string[],
): Set<number> {
  const exempt = new Set<number>();
  let inFence = false;
  for (let i = 0; i < sourceLines.length; i++) {
    const src = sourceLines[i] ?? "";
    if (!isMaskedCommentLine(src, maskedLines[i] ?? "")) continue;
    const trimmed = src.trim();
    if (inFence) {
      exempt.add(i + 1);
      if (FENCE_CLOSE.test(trimmed)) inFence = false;
    } else if (COMPILE_FAIL_FENCE_OPEN.test(trimmed)) {
      inFence = true;
      exempt.add(i + 1);
    }
  }
  return exempt;
}

function countByMask(source: string, masked: string, compileFailFence = false): Counted {
  const sourceLines = source.split("\n");
  const maskedLines = masked.split("\n");
  const lines: CommentLine[] = [];
  let safetyExempt = 0;
  let compileFailExempt = 0;
  const fenceExempt = compileFailFence
    ? compileFailFenceExemptLines(sourceLines, maskedLines)
    : undefined;
  for (let i = 0; i < sourceLines.length; i++) {
    const src = sourceLines[i] ?? "";
    if (!isMaskedCommentLine(src, maskedLines[i] ?? "")) continue;
    if (fenceExempt?.has(i + 1)) {
      compileFailExempt++;
      continue;
    }
    const trimmed = src.trim();
    if (SAFETY_FORM.test(trimmed)) {
      safetyExempt++;
      continue;
    }
    lines.push({ line: i + 1, text: trimmed });
  }
  return {
    count: lines.length,
    lines,
    safetyExempt,
    shebangExempt: 0,
    compileFailExempt,
  };
}

function countLineLeading(source: string, allowShebang: boolean): Counted {
  const sourceLines = source.split("\n");
  const lines: CommentLine[] = [];
  let shebangExempt = 0;
  for (let i = 0; i < sourceLines.length; i++) {
    const raw = sourceLines[i] ?? "";
    if (!LINE_LEADING_HASH.test(raw)) continue;
    const trimmed = raw.trim();
    if (allowShebang && i === 0 && SHEBANG.test(trimmed)) {
      shebangExempt++;
      continue;
    }
    lines.push({ line: i + 1, text: trimmed });
  }
  return { count: lines.length, lines, safetyExempt: 0, shebangExempt, compileFailExempt: 0 };
}

function countPackageJson(source: string): Counted {
  const sourceLines = source.split("\n");
  const lines: CommentLine[] = [];
  for (let i = 0; i < sourceLines.length; i++) {
    const raw = sourceLines[i] ?? "";
    if (!PACKAGE_JSON_COMMENT_KEY.test(raw)) continue;
    lines.push({ line: i + 1, text: raw.trim() });
  }
  return { count: lines.length, lines, safetyExempt: 0, shebangExempt: 0, compileFailExempt: 0 };
}

function scan(root: string): ScanResult {
  const keys = deriveKeys(root);
  const stats = new Map<string, KeyStats>(keys.map((key) => [key, { actual: 0, files: [] }]));
  const unmatched: string[] = [];
  let total = 0;
  let safetyExempt = 0;
  let shebangExempt = 0;
  let compileFailExempt = 0;
  let fileCount = 0;
  const walked = walkTextFiles(root, "", { extensions: WALK_EXTENSIONS, skipDirs: SKIP_DIRS });
  for (const file of walked) {
    if (file.rel.startsWith(CORPUS_DIR_PREFIX)) continue;
    const name = file.rel.slice(file.rel.lastIndexOf("/") + 1);
    const face = faceOf(name);
    if (face === undefined) continue;
    const key = matchKey(file.rel, keys);
    if (key === undefined) {
      unmatched.push(file.rel);
      continue;
    }
    fileCount++;
    const source = readFileSync(file.abs, "utf8");
    let counted: Counted;
    if (face === "rust") counted = countByMask(source, maskNonCode(source, true), true);
    else if (face === "script") counted = countByMask(source, maskComments(source));
    else if (face === "shell") counted = countLineLeading(source, true);
    else if (face === "toml") counted = countLineLeading(source, false);
    else counted = countPackageJson(source);
    safetyExempt += counted.safetyExempt;
    shebangExempt += counted.shebangExempt;
    compileFailExempt += counted.compileFailExempt;
    if (counted.count === 0) continue;
    total += counted.count;
    const stat = stats.get(key);
    if (stat === undefined) continue;
    stat.actual += counted.count;
    stat.files.push({ rel: file.rel, count: counted.count, lines: counted.lines });
  }
  return {
    keys,
    stats,
    unmatched,
    total,
    safetyExempt,
    shebangExempt,
    compileFailExempt,
    fileCount,
  };
}

function fatalBaseline(message: string): Baseline {
  return { fatal: message, missing: [], extra: [], invalid: [], values: new Map() };
}

function loadBaseline(baselinePath: string, keys: readonly string[]): Baseline {
  if (!existsSync(baselinePath)) {
    return fatalBaseline(
      `基线文件不存在：${baselinePath}（再生成：bun scripts/check-comment-gate.ts --write-baseline）`,
    );
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(readFileSync(baselinePath, "utf8"));
  } catch (error) {
    return fatalBaseline(`基线文件不是合法 JSON：${baselinePath}（${String(error)}）`);
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    return fatalBaseline(`基线文件顶层必须是对象（目录键 → 非负整数计数）：${baselinePath}`);
  }
  const entries = Object.entries(parsed as Record<string, unknown>);
  const keySet = new Set(keys);
  const givenKeys = new Set(entries.map(([key]) => key));
  const values = new Map<string, number>();
  for (const [key, value] of entries) {
    if (keySet.has(key) && typeof value === "number") values.set(key, value);
  }
  return {
    missing: keys.filter((key) => !givenKeys.has(key)),
    extra: entries.map(([key]) => key).filter((key) => !keySet.has(key)),
    invalid: entries.filter(
      ([, value]) => typeof value !== "number" || !Number.isInteger(value) || value < 0,
    ),
    values,
  };
}

function writeBaselineFile(baselinePath: string, result: ScanResult): void {
  const entries = result.keys.map((key) => [key, result.stats.get(key)?.actual ?? 0]);
  const body = JSON.stringify(Object.fromEntries(entries), null, 2) + "\n";
  mkdirSync(join(baselinePath, ".."), { recursive: true });
  writeFileSync(baselinePath, body);
}

function main(): void {
  const args = process.argv.slice(2);
  const writeBaseline = args.includes("--write-baseline");
  const positional = args.filter((arg) => !arg.startsWith("--"));
  if (positional.length > 1) fail("至多一个位置参数（扫描根目录）");
  const root = positional[0] !== undefined ? resolve(positional[0]) : DEFAULT_ROOT;
  if (!existsSync(root)) fail(`扫描根目录不存在：${root}`);
  const result = scan(root);
  const problems: string[] = [];
  if (result.fileCount === 0) {
    problems.push(`✗ 扫描面为空（${root} 下无受辖文件）——拒绝以空集假绿通过`);
  }
  for (const rel of result.unmatched) {
    problems.push(
      `✗ 未登记目录键：${rel}——扫描文件必须落在基线键集内，新 crate/包/根文件须显式登记基线（ADR-0145 决策 4）`,
    );
  }
  const baselinePath = join(root, ...BASELINE_REL_SEGMENTS);
  if (writeBaseline) {
    if (problems.length > 0) {
      for (const problem of problems) console.error(problem);
      console.error(`❌ 基线写入失败：${problems.length} 处结构问题（ADR-0145 / issue #2202）`);
      process.exit(1);
    }
    writeBaselineFile(baselinePath, result);
    console.log(
      `✓ 基线已写入 ${baselinePath}：${result.keys.length} 键、共 ${result.total} 行` +
        `（豁免：SAFETY ${result.safetyExempt} 行、shebang ${result.shebangExempt} 行、` +
        `compile_fail 载体 ${result.compileFailExempt} 行）` +
        `（ADR-0145 / issue #2202）`,
    );
    return;
  }
  const baseline = loadBaseline(baselinePath, result.keys);
  if (baseline.fatal !== undefined) problems.push(`✗ ${baseline.fatal}`);
  problems.push(
    ...baseline.missing.map(
      (key) => `✗ 基线缺键：${key}（基线键集须与扫描键集双向全等，ADR-0145 决策 4）`,
    ),
  );
  problems.push(
    ...baseline.extra.map(
      (key) => `✗ 基线冗余键：${key}（键在扫描面已不存在——目录删除/改名后须同步基线）`,
    ),
  );
  problems.push(
    ...baseline.invalid.map(
      ([key, value]) =>
        `✗ 基线非法计数：${key} = ${JSON.stringify(value)}（须为非负整数，ADR-0145 决策 4）`,
    ),
  );
  let overCount = 0;
  let zeroCount = 0;
  const schemaClean =
    baseline.fatal === undefined &&
    baseline.missing.length === 0 &&
    baseline.extra.length === 0 &&
    baseline.invalid.length === 0;
  if (schemaClean) {
    for (const key of result.keys) {
      const stat = result.stats.get(key);
      if (stat === undefined) continue;
      const limit = baseline.values.get(key) ?? 0;
      if (stat.actual <= limit) continue;
      if (limit === 0) {
        zeroCount++;
        problems.push(`✗ 零容忍命中（基线 0）：${key}`);
        for (const file of stat.files) {
          for (const commentLine of file.lines) {
            problems.push(`    ${file.rel}:${commentLine.line}：${commentLine.text}`);
          }
        }
      } else {
        overCount++;
        problems.push(
          `✗ 棘轮超线：${key}：实际 ${stat.actual} 行 > 基线 ${limit} 行（+${stat.actual - limit}）`,
        );
        for (const file of stat.files) {
          problems.push(`    ${file.rel}：${file.count} 行`);
        }
      }
    }
  }
  if (problems.length > 0) {
    for (const problem of problems) console.error(problem);
    console.error(
      `❌ 注释禁令失败：${overCount} 键棘轮超线 / ${zeroCount} 键零容忍命中 / ` +
        `${problems.length} 处问题；注释只许删不许加，` +
        `豁免闭集 = // SAFETY: 形态、shebang 行与 compile_fail doctest 载体，` +
        `规则与基线纪律见 ADR-0145 / issue #2202`,
    );
    process.exit(1);
  }
  console.log(
    `✓ 注释棘轮通过：${result.keys.length} 键、${result.total} 行注释在基线内` +
      `（SAFETY 豁免 ${result.safetyExempt} 行、shebang 豁免 ${result.shebangExempt} 行、` +
      `compile_fail 载体豁免 ${result.compileFailExempt} 行）；` +
      `基线 ${BASELINE_REL_SEGMENTS.join("/")} 只许降不许升、归零即零容忍（ADR-0145 / issue #2202）`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
