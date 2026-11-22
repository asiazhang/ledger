#!/usr/bin/env bun

import { existsSync, readFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { pathToFileURL } from "node:url";
import {
  lineAt,
  maskNonCode,
  readDirEntries,
  RUST_EXTENSIONS,
  walkTextFiles,
} from "./gate-primitives.ts";

export const FIXED_NOW_REGISTRY_REL = "src/test_support/mod.rs";

function fail(message: string): never {
  console.error(`✗ Rust 测试守门：${message}`);
  process.exit(1);
}

const CHAR_LITERAL_RE = /^'(\\(?:.|x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]{1,6}\})|[^\\'])'/;

function stringEndAt(source: string, i: number): number {
  const isRaw = source[i] === "r" && (source[i + 1] === '"' || source[i + 1] === "#");
  const isByteRaw =
    source[i] === "b" && source[i + 1] === "r" && (source[i + 2] === '"' || source[i + 2] === "#");
  if (source[i] !== '"' && source[i] !== "b" && !isRaw && !isByteRaw) return -1;
  let j = i;
  let rawHashes = 0;
  if (source[j] === "b") j++;
  if (source[j] === "r") {
    j++;
    while (source[j] === "#") {
      rawHashes++;
      j++;
    }
  }
  if (source[j] !== '"') return -1;
  j++;
  const terminator = '"' + "#".repeat(rawHashes);
  while (j < source.length) {
    if (rawHashes === 0 && source[j] === "\\") {
      j += 2;
      continue;
    }
    if (source.startsWith(terminator, j)) return j + terminator.length;
    j++;
  }
  return source.length;
}

function charEndAt(source: string, i: number): number {
  const m = CHAR_LITERAL_RE.exec(source.slice(i, i + 12));
  return m ? i + m[0].length : -1;
}

interface Region {
  start: number;
  end: number;
}

function extractCfgTestRegions(masked: string): Region[] {
  const regions: Region[] = [];
  let i = 0;
  while ((i = masked.indexOf("#[cfg(test)]", i)) !== -1) {
    i += "#[cfg(test)]".length;
    for (;;) {
      while (i < masked.length && /\s/.test(masked[i])) i++;
      if (masked[i] === "#") {
        const close = masked.indexOf("]", i);
        if (close === -1) break;
        i = close + 1;
      } else break;
    }
    const mod = /^mod\s+\w+\s*\{/.exec(masked.slice(i, i + 64));
    if (!mod) continue;
    const bodyStart = i + mod[0].length;
    let depth = 1;
    let j = bodyStart;
    while (j < masked.length && depth > 0) {
      const strEnd = stringEndAt(masked, j);
      if (strEnd !== -1) {
        j = strEnd;
        continue;
      }
      if (masked[j] === "'") {
        j = Math.max(charEndAt(masked, j), j + 1);
        continue;
      }
      const c = masked[j];
      if (c === "{") depth++;
      else if (c === "}") depth--;
      j++;
    }
    if (depth !== 0) continue;
    regions.push({ start: i, end: j });
  }
  return regions;
}

const RULE1_IDENTIFIERS = /\b(open_in_memory|init_db)\b/g;
const RULE3_DECLARATION = /pub\s+const\s+\w+\s*:\s*&str\s*=\s*"([^"]+)"/g;
const RULE4_LITERAL_CONSTRUCTIONS = [/\bSegmentEntry\s*\{/g, /\bChannelManifest\s*\{/g];
const RULE4_DIGEST_IDENTIFIERS = /\b(Sha256|sha2)\b/g;
const RULE4_CHANNEL_SURFACE = [
  /\bChannelManifest\b/,
  /\bSegmentEntry\b/,
  /\bEnvelopeMode\b/,
  /\bpublish_raw_segment\b/,
];

interface E2eException {
  file: string;
  rule: Rule;
  count: number;
  why: string;
}
const E2E_REGISTERED_EXCEPTIONS: readonly E2eException[] = [
  {
    file: "tests/e2e/instruments_steps.rs",
    rule: 2,
    count: 3,
    why: "存量同步行夹具：公开创建入口只产 manual 行，来源随行终身不变（ADR-0036），同步来源拒删/upsert 来源不改写的被测前提依赖直置",
  },
  {
    file: "tests/e2e/investment_trend_steps.rs",
    rule: 2,
    count: 1,
    why: "汇率历史周采样无公开落库入口：唯一写入点 sync 持久化为 pub(super) 模块私有（采集通道需 HTTP），域层无公开入口",
  },
  {
    file: "tests/e2e/transactions_query_steps.rs",
    rule: 3,
    count: 4,
    why: "两处直置行 SQL（同日批量导入 ×1、播种 8 类 ×1）各含 created_at/updated_at 同值字面量两次：同 created_at 平局是被测前提（确定性排序 id tiebreaker），行为层逐行发放秒级时钟、跨秒即失去平局，前提无法确定性构造",
  },
];

interface Hit {
  rule: Rule;
  line: number;
}

const RULES = [1, 2, 3, 4] as const;
type Rule = (typeof RULES)[number];

function findHits(
  masked: string,
  lineOf: (offset: number) => number,
  bannedTables: string[],
  fixedNowValues: string[],
): Hit[] {
  const hits: Hit[] = [];
  RULE1_IDENTIFIERS.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = RULE1_IDENTIFIERS.exec(masked))) hits.push({ rule: 1, line: lineOf(m.index) });
  if (bannedTables.length > 0) {
    const re2 = new RegExp(`INSERT\\s+INTO\\s+(?:${bannedTables.join("|")})\\b`, "gi");
    let m2: RegExpExecArray | null;
    while ((m2 = re2.exec(masked))) hits.push({ rule: 2, line: lineOf(m2.index) });
  }
  for (const value of fixedNowValues) {
    let at = masked.indexOf(value);
    while (at !== -1) {
      hits.push({ rule: 3, line: lineOf(at) });
      at = masked.indexOf(value, at + value.length);
    }
  }
  for (const re of RULE4_LITERAL_CONSTRUCTIONS) {
    re.lastIndex = 0;
    let m4: RegExpExecArray | null;
    while ((m4 = re.exec(masked))) hits.push({ rule: 4, line: lineOf(m4.index) });
  }
  if (RULE4_CHANNEL_SURFACE.some((re) => re.test(masked))) {
    RULE4_DIGEST_IDENTIFIERS.lastIndex = 0;
    let m5: RegExpExecArray | null;
    while ((m5 = RULE4_DIGEST_IDENTIFIERS.exec(masked)))
      hits.push({ rule: 4, line: lineOf(m5.index) });
  }
  return hits;
}

function walkRustFiles(dir: string): string[] {
  return walkTextFiles(dir, "", { extensions: RUST_EXTENSIONS }).map((f) => f.abs);
}

function memberCrateRustRoots(srcTauri: string): string[] {
  const cratesDir = join(srcTauri, "crates");
  if (!existsSync(cratesDir)) return [];
  const roots: string[] = [];
  for (const entry of readDirEntries(cratesDir)) {
    if (!entry.isDirectory) continue;
    for (const sub of ["src", "tests"]) {
      const dir = join(cratesDir, entry.name, sub);
      if (existsSync(dir)) roots.push(dir);
    }
  }
  return roots;
}

function isTestPath(relSegments: string[]): boolean {
  return (
    relSegments.slice(0, -1).includes("tests") || relSegments[relSegments.length - 1] === "tests.rs"
  );
}

function isThinShell(relSegments: string[]): boolean {
  const base = relSegments[relSegments.length - 1];
  return (
    relSegments[relSegments.length - 2] === "tests" &&
    (base === "common.rs" || base === "batch_common.rs")
  );
}

function extractSeedTables(seedRsPath: string): string[] {
  if (!existsSync(seedRsPath)) {
    fail(`工厂种子登记处缺失：${seedRsPath}（统一测试数据库工厂，issue #751 / ADR-0084）`);
  }
  const source = readFileSync(seedRsPath, "utf8");
  const tables = [...new Set([...source.matchAll(/INSERT\s+INTO\s+(\w+)/gi)].map((m) => m[1]))];
  if (tables.length === 0) {
    fail(`种子登记处 ${seedRsPath} 提不出任何 INSERT INTO 表名（清单漂移？）`);
  }
  return tables;
}

function extractFixedNowValues(registryPath: string): string[] {
  if (!existsSync(registryPath)) {
    fail(`固定时刻登记处缺失：${registryPath}（FIXED_NOW 唯一住址，ADR-0084 决策 5）`);
  }
  const masked = maskNonCode(readFileSync(registryPath, "utf8"), true);
  const values = [...new Set([...masked.matchAll(RULE3_DECLARATION)].map((m) => m[1]))];
  if (values.length === 0) {
    fail(`固定时刻登记处 ${registryPath} 提不出任何 pub const &str 现值（登记处漂移？）`);
  }
  return values;
}

function main(): void {
  const srcTauri = process.argv[2]
    ? resolve(process.argv[2])
    : join(fileURLToPath(import.meta.url), "..", "..", "src-tauri");
  const srcDir = join(srcTauri, "src");
  const testsDir = join(srcTauri, "tests");
  if (!existsSync(srcDir)) fail(`src-tauri 目录不存在：${srcTauri}`);

  const bannedTables = extractSeedTables(join(srcDir, "test_support", "seed.rs"));
  const fixedNowValues = extractFixedNowValues(join(srcTauri, FIXED_NOW_REGISTRY_REL));

  const files = [
    ...walkRustFiles(srcDir),
    ...(existsSync(testsDir) ? walkRustFiles(testsDir) : []),
    ...memberCrateRustRoots(srcTauri).flatMap((dir) => walkRustFiles(dir)),
  ];
  const { countByFile, hits } = scanFiles(files, srcTauri, bannedTables, fixedNowValues);
  const scanned = new Set(files.map((f) => relative(srcTauri, f).split("\\").join("/")));
  const problems = violations(countByFile, hits, scanned);
  if (problems.length > 0) {
    console.error(
      `✗ Rust 测试守门：发现 ${problems.length} 处违规（纯禁令，无白名单；` +
        `禁用种子表：${bannedTables.join(" ")}；禁用时刻值：${fixedNowValues.join(" ")}）\n` +
        problems.join("\n") +
        `\n建库/种子/断言唯一入口：src-tauri/src/test_support/（spec #728 / issue #751，ADR-0084）`,
    );
    process.exit(1);
  }

  console.log(
    `✅ Rust 测试守门通过（纯禁令：白名单机制已随 #758 收口移除；` +
      `禁用种子表 ${bannedTables.length} 张：${bannedTables.join(" ")}；` +
      `禁用时刻值 ${fixedNowValues.length} 个：${fixedNowValues.join(" ")}；` +
      `tests/e2e 已登记例外 ${E2E_REGISTERED_EXCEPTIONS.length} 条，严格相等校验）`,
  );
}

function scanFiles(
  files: string[],
  srcTauri: string,
  bannedTables: string[],
  fixedNowValues: string[],
): { countByFile: Map<string, Map<Rule, number>>; hits: Array<{ rel: string } & Hit> } {
  const countByFile = new Map<string, Map<Rule, number>>();
  const hits: Array<{ rel: string } & Hit> = [];
  for (const file of files.sort()) {
    const rel = relative(srcTauri, file).split("\\").join("/");
    const segments = rel.split("/");
    const underTestSupport = segments[0] === "src" && segments[1] === "test_support";
    const inE2e = rel.startsWith("tests/e2e/");
    const source = readFileSync(file, "utf8");
    const masked = maskNonCode(source, true);

    const regions: Region[] = isTestPath(segments)
      ? [{ start: 0, end: masked.length }]
      : extractCfgTestRegions(masked);
    if (regions.length === 0) continue;

    const lineOf = (offset: number): number => lineAt(masked, offset);

    for (const region of regions) {
      const text = masked.slice(region.start, region.end);
      for (const hit of findHits(
        text,
        (o) => lineOf(region.start + o),
        bannedTables,
        fixedNowValues,
      )) {
        if (underTestSupport) continue;
        if (inE2e && hit.rule === 1) continue;
        if (hit.rule === 2 && isThinShell(segments)) continue;
        if (!countByFile.has(rel)) countByFile.set(rel, new Map());
        const byRule = countByFile.get(rel)!;
        byRule.set(hit.rule, (byRule.get(hit.rule) ?? 0) + 1);
        hits.push({ rel, ...hit });
      }
    }
  }
  return { countByFile, hits };
}

function violations(
  countByFile: Map<string, Map<Rule, number>>,
  hits: Array<{ rel: string } & Hit>,
  scanned: Set<string>,
): string[] {
  const problems: string[] = [];
  const RULE_NAMES = {
    1: "直连建库",
    2: "夹具裸SQL",
    3: "默认时刻字面量",
    4: "自建通道线格式",
  } as const;
  for (const file of [...countByFile.keys()].sort()) {
    for (const rule of RULES) {
      const actual = countByFile.get(file)?.get(rule) ?? 0;
      if (actual === 0) continue;
      const lines = hits
        .filter((h) => h.rel === file && h.rule === rule)
        .map((h) => h.line)
        .sort((a, b) => a - b);
      if (file.startsWith("tests/e2e/")) {
        const registered = E2E_REGISTERED_EXCEPTIONS.find(
          (e) => e.file === file && e.rule === rule,
        );
        if (registered === undefined) {
          problems.push(
            `  ${file}:${lines.join(":")}\n` +
              `      规则 ${rule}（${RULE_NAMES[rule]}）命中 ${actual} 处——未登记例外（登记处 ADR-0086 修订注记）：` +
              `公开入口存在则收敛，不存在则代码处附动机注释并登记后恢复扫描`,
          );
        } else if (actual !== registered.count) {
          problems.push(
            `  ${file}:${lines.join(":")}\n` +
              `      规则 ${rule}（${RULE_NAMES[rule]}）实际命中 ${actual} 处 ≠ 登记的 ${registered.count} 处` +
              `（${registered.why}）——命中数漂移即例外失真，请复核后同步更新代码注释、例外表与 ADR 登记`,
          );
        }
        continue;
      }
      problems.push(
        `  ${file}:${lines.join(":")}\n` +
          `      规则 ${rule}（${RULE_NAMES[rule]}）命中 ${actual} 处——纯禁令（白名单已移除），` +
          `收敛到 test_support 工厂/种子或域薄皮（ADR-0084）`,
      );
    }
  }
  for (const e of E2E_REGISTERED_EXCEPTIONS) {
    if (!scanned.has(e.file)) continue;
    const actual = countByFile.get(e.file)?.get(e.rule) ?? 0;
    if (actual === 0) {
      problems.push(
        `  ${e.file}\n` +
          `      规则 ${e.rule}（${RULE_NAMES[e.rule]}）已登记例外 ${e.count} 处实际命中 0 处——例外已收敛，` +
          `请同步删除例外表条目、代码处注释与 ADR 登记`,
      );
    }
  }
  return problems;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
