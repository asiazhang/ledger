#!/usr/bin/env bun

import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { lineAt, walkTextFiles } from "./gate-primitives.ts";

const DEFAULT_ROOT = join(fileURLToPath(import.meta.url), "..", "..");

const SELF_REL = "scripts/check-eastmoney-residue.ts";
const WRAPPER_TEST_REL = "scripts/check-eastmoney-residue.test.ts";

const SCAN_EXTENSIONS = new Set([
  ".rs",
  ".ts",
  ".tsx",
  ".vue",
  ".md",
  ".json",
  ".sql",
  ".sh",
  ".html",
  ".toml",
  ".yml",
  ".yaml",
  ".feature",
]);

const SKIP_DIRS = new Set(["node_modules", "target", ".git", "docs", "dist", "coverage"]);

const SKIP_FILES = new Set(["CHANGELOG.md", SELF_REL, WRAPPER_TEST_REL]);

function fail(message: string): never {
  console.error(`✗ 东财行情面零残留守门：${message}`);
  process.exit(1);
}

export interface ResidueHit {
  file: string;
  line: number;
  marker: string;
  text: string;
}

export interface ResidueException {
  file: string;
  count: number;
  why: string;
}

export const REGISTERED_EXCEPTIONS: readonly ResidueException[] = [];

export const BANNED_MARKERS: readonly string[] = ["eastmoney.com", "dfcfw", "1234567.com.cn"];

function markerPattern(marker: string): RegExp {
  return new RegExp(marker.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "gi");
}

export function scanResidue(rel: string, source: string): ResidueHit[] {
  const hits: ResidueHit[] = [];
  const lines = source.split("\n");
  for (const marker of BANNED_MARKERS) {
    for (const m of source.matchAll(markerPattern(marker))) {
      const line = lineAt(source, m.index);
      hits.push({ file: rel, line, marker, text: (lines[line - 1] ?? "").trim() });
    }
  }
  return hits.sort((a, b) => a.line - b.line || a.marker.localeCompare(b.marker));
}

function main(): void {
  const root = process.argv[2] ? resolve(process.argv[2]) : DEFAULT_ROOT;
  if (!existsSync(root)) fail(`扫描根不存在：${root}`);

  for (const rel of [SELF_REL, WRAPPER_TEST_REL]) {
    if (!existsSync(join(root, rel))) {
      fail(`豁免清单漂移：${rel} 不存在——守门或其包装测试被改名/搬迁，请同步 SKIP_FILES 登记`);
    }
  }

  const scanned = walkTextFiles(root, "", {
    extensions: SCAN_EXTENSIONS,
    skipDirs: SKIP_DIRS,
    skipFiles: SKIP_FILES,
  });
  const hits = scanned.flatMap((f) => scanResidue(f.rel, readFileSync(f.abs, "utf8")));
  const byFile = new Map<string, ResidueHit[]>();
  for (const hit of hits) {
    const list = byFile.get(hit.file) ?? [];
    list.push(hit);
    byFile.set(hit.file, list);
  }

  const problems: string[] = [];
  const registered = new Map(REGISTERED_EXCEPTIONS.map((e) => [e.file, e]));
  for (const [file, list] of byFile) {
    if (registered.has(file)) continue;
    for (const hit of list) {
      problems.push(
        `✗ ${file}:${hit.line} 命中禁令标记「${hit.marker}」：${hit.text}\n` +
          `    —— 东财行情面端点已随 ADR-0130 决策 1 删除，不得回归（守门判据①）`,
      );
    }
  }
  for (const e of REGISTERED_EXCEPTIONS) {
    const actual = byFile.get(e.file);
    if (!actual && !existsSync(join(root, e.file))) {
      problems.push(
        `✗ 例外清单漂移：${e.file} 不存在——${e.why}，请复核后更新 REGISTERED_EXCEPTIONS`,
      );
      continue;
    }
    const count = actual?.length ?? 0;
    if (count === 0) {
      problems.push(
        `✗ 例外已收敛：${e.file} 零命中——${e.why}；删除 REGISTERED_EXCEPTIONS 登记条目，` +
          "守门收紧到零例外",
      );
    } else if (count !== e.count) {
      problems.push(
        `✗ 例外计数漂移：${e.file} 实际命中 ${count} 处 ≠ 登记 ${e.count} 处——` +
          `${e.why}；逐条复核 ${fileHitsLabel(actual)} 后同步登记`,
      );
    }
  }

  if (problems.length > 0) {
    console.error(`✗ 东财行情面零残留守门：${problems.length} 处问题\n${problems.join("\n")}`);
    process.exit(1);
  }
  console.log(
    `✓ 东财行情面零残留守门通过：扫描 ${scanned.length} 个文件，未登记端点域名零命中；` +
      `已登记例外 ${REGISTERED_EXCEPTIONS.length} 条` +
      `${REGISTERED_EXCEPTIONS.length > 0 ? `（${REGISTERED_EXCEPTIONS.map((e) => e.file).join("、")}）` : "（零例外）"}`,
  );
}

function fileHitsLabel(hits?: ResidueHit[]): string {
  return (hits ?? []).map((h) => `${h.file}:${h.line}`).join("、");
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
