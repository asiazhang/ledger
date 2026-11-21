#!/usr/bin/env bun

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import { maskNonCode, readDirEntries, RUST_EXTENSIONS, walkTextFiles } from "./gate-primitives.ts";

export const SOURCE_LOCALE_DIR = "zh-CN";

export function flattenKeys(obj: unknown, prefix = ""): string[] {
  const keys: string[] = [];
  for (const [k, v] of Object.entries(obj as Record<string, unknown>)) {
    const path = prefix ? `${prefix}.${k}` : k;
    if (v !== null && typeof v === "object" && !Array.isArray(v)) {
      keys.push(...flattenKeys(v, path));
    } else {
      keys.push(path);
    }
  }
  return keys.sort();
}

export interface LocaleFailure {
  locale: string;
  sourceOnly: string[];
  otherOnly: string[];
}

export function diffKeySets(
  sourceKeys: string[],
  otherKeys: string[],
): { sourceOnly: string[]; otherOnly: string[] } {
  const source = new Set(sourceKeys);
  const other = new Set(otherKeys);
  return {
    sourceOnly: sourceKeys.filter((k) => !other.has(k)),
    otherOnly: otherKeys.filter((k) => !source.has(k)),
  };
}

export function collectLocaleKeys(dir: string): string[] {
  const keys: string[] = [];
  for (const entry of readDirEntries(dir)) {
    if (!entry.name.endsWith(".json")) continue;
    const domain = entry.name.slice(0, -".json".length);
    const parsed: unknown = JSON.parse(readFileSync(join(dir, entry.name), "utf-8"));
    keys.push(...flattenKeys(parsed, domain));
  }
  return keys.sort();
}

const CODED_CALL_RE = /\b(codedp_not_found|coded_not_found|codedp|coded)\s*\(/g;
const RUST_STR_CONST_RE = /\bconst\s+([A-Z_][A-Z0-9_]*)\s*:\s*&str\s*=\s*"([^"]*)"/g;

export function stripCfgTestBlocks(src: string): string {
  let out = src;
  for (;;) {
    const m = /#\[\s*cfg\s*\(\s*test\s*\)\s*\]/.exec(out);
    if (!m || m.index === undefined) break;
    const brace = out.indexOf("{", m.index + m[0].length);
    if (brace === -1) {
      out = out.slice(0, m.index);
      break;
    }
    let depth = 0;
    let i = brace;
    for (; i < out.length; i++) {
      if (out[i] === "{") depth += 1;
      else if (out[i] === "}") {
        depth -= 1;
        if (depth === 0) break;
      }
    }
    out = out.slice(0, m.index) + out.slice(i + 1);
  }
  return out;
}

export function isRustTestPath(relPath: string): boolean {
  const parts = relPath.split("/");
  const file = parts[parts.length - 1] ?? "";
  if (parts.slice(0, -1).some((p) => p === "tests" || p.endsWith("_tests"))) return true;
  return file.endsWith("tests.rs") || file.endsWith("_tests.rs");
}

export interface RustCodedCodesResult {
  codes: string[];
  unresolvedCallSites: number;
}

function walkRustFiles(root: string): string[] {
  return walkTextFiles(root, "", {
    extensions: RUST_EXTENSIONS,
    skipDirs: new Set(["target"]),
  }).map((f) => f.abs);
}

export function collectRustCodedErrorCodes(rustRoot: string): RustCodedCodesResult {
  const constTable = new Map<string, string>();
  const sources: string[] = [];
  for (const path of walkRustFiles(rustRoot)) {
    const rel = path.slice(rustRoot.length + 1);
    if (isRustTestPath(rel)) continue;
    const src = stripCfgTestBlocks(maskNonCode(readFileSync(path, "utf-8"), true));
    sources.push(src);
    for (const m of src.matchAll(RUST_STR_CONST_RE)) {
      const name = m[1];
      const value = m[2];
      if (name !== undefined && value !== undefined && !constTable.has(name)) {
        constTable.set(name, value);
      }
    }
  }
  const codes = new Set<string>();
  let unresolvedCallSites = 0;
  for (const src of sources) {
    for (const m of src.matchAll(CODED_CALL_RE)) {
      const before = src.slice(Math.max(0, (m.index ?? 0) - 30), m.index);
      if (/\bfn\s+$/.test(before)) continue;
      let j = (m.index ?? 0) + m[0].length;
      while (
        j < src.length &&
        (src[j] === " " || src[j] === "\t" || src[j] === "\r" || src[j] === "\n")
      )
        j += 1;
      const ch = src[j];
      if (ch === '"') {
        const end = src.indexOf('"', j + 1);
        if (end !== -1) {
          const code = src.slice(j + 1, end);
          if (code) codes.add(code);
          continue;
        }
      }
      const ident = /^[A-Z_][A-Z0-9_]*/.exec(src.slice(j));
      if (ident) {
        const value = constTable.get(ident[0]);
        if (value !== undefined) {
          if (value) codes.add(value);
          continue;
        }
      }
      unresolvedCallSites += 1;
    }
  }
  return { codes: [...codes].sort(), unresolvedCallSites };
}

export function diffCodedCoverage(codes: string[], errorTemplateKeys: string[]): string[] {
  const keys = new Set(errorTemplateKeys);
  return codes.filter((c) => !keys.has(c));
}

export interface CompareResult {
  sourceLocale: string;
  locales: string[];
  failures: LocaleFailure[];
}

export function compareLocalesDir(localesDir: string): CompareResult {
  const dirs = readDirEntries(localesDir)
    .filter((e) => e.isDirectory)
    .map((e) => e.name);
  const sourceKeys = collectLocaleKeys(join(localesDir, SOURCE_LOCALE_DIR));
  const failures: LocaleFailure[] = [];
  for (const locale of dirs) {
    if (locale === SOURCE_LOCALE_DIR) continue;
    const diff = diffKeySets(sourceKeys, collectLocaleKeys(join(localesDir, locale)));
    if (diff.sourceOnly.length > 0 || diff.otherOnly.length > 0) {
      failures.push({ locale, ...diff });
    }
  }
  return { sourceLocale: SOURCE_LOCALE_DIR, locales: dirs, failures };
}

function main(): void {
  const localesDir =
    process.argv[2] ?? fileURLToPath(new URL("../packages/i18n/src/locales", import.meta.url));
  const rustRoot = process.argv[3] ?? fileURLToPath(new URL("../src-tauri", import.meta.url));
  const { sourceLocale, locales, failures } = compareLocalesDir(localesDir);
  if (failures.length === 0) {
    console.log(
      `✅ i18n key 全等：${locales.join(" / ")} 各语言 key 集合与源语言 ${sourceLocale} 全等`,
    );
  } else {
    for (const f of failures) {
      console.error(`✗ locale「${f.locale}」与源语言「${sourceLocale}」key 集合不一致：`);
      for (const key of f.sourceOnly) console.error(`  - 缺失：${key}`);
      for (const key of f.otherOnly) console.error(`  - 多余：${key}`);
    }
    process.exit(1);
  }

  const { codes, unresolvedCallSites } = collectRustCodedErrorCodes(rustRoot);
  const zhErrors = flattenKeys(
    JSON.parse(readFileSync(join(localesDir, "zh-CN", "errors.json"), "utf-8")) as unknown,
  );
  const enErrors = flattenKeys(
    JSON.parse(readFileSync(join(localesDir, "en-US", "errors.json"), "utf-8")) as unknown,
  );
  const missing = [...diffCodedCoverage(codes, zhErrors), ...diffCodedCoverage(codes, enErrors)];
  if (missing.length > 0) {
    console.error(
      `✗ 码化错误模板缺失（ADR-0050：新增错误条件 = 码化构造器 + errors.json 两份补翻译）：`,
    );
    for (const code of new Set(missing)) console.error(`  - 缺模板：${code}`);
    process.exit(1);
  }
  const unresolvedNote =
    unresolvedCallSites > 0
      ? `（另有 ${unresolvedCallSites} 处动态码构造点无法静态枚举，漏报方向安全）`
      : "";
  console.log(
    `✅ 码化错误模板覆盖：${codes.length} 个码化错误码在 zh-CN/en-US errors.json 均有模板${unresolvedNote}`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
