#!/usr/bin/env bun

import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import {
  lineAt,
  maskNonCode,
  readDirEntries,
  RUST_EXTENSIONS,
  walkTextFiles,
} from "./gate-primitives.ts";
import { INFRA_SRC_REL } from "./check-structure.ts";

const DEFAULT_SRC_TAURI = join(fileURLToPath(import.meta.url), "..", "..", "src-tauri");

function fail(message: string): never {
  console.error(`✗ 基础设施账本数据表 DML 禁令：${message}`);
  process.exit(1);
}

export interface MechanismTable {
  table: string;
  why: string;
}

export const MECHANISM_TABLES: readonly MechanismTable[] = [
  {
    table: "app_settings",
    why: "设置 KV 归口（crate::settings 单点收口，ADR-0111 共享接缝）——设置非账本数据",
  },
  {
    table: "sync_device",
    why: "多端同步协议表（协议 crate 唯一 SQL 收口，#1089 / ADR-0091）——同步机制非账本数据",
  },
  {
    table: "sync_ops",
    why: "多端同步协议表（协议 crate 唯一 SQL 收口，#1089 / ADR-0091）——同步机制非账本数据",
  },
  {
    table: "sync_parked_ops",
    why: "多端同步协议表（协议 crate 唯一 SQL 收口，#1089 / ADR-0091）——同步机制非账本数据",
  },
  {
    table: "sync_stream_positions",
    why: "多端同步协议表（协议 crate 唯一 SQL 收口，#1089 / issue #857）——同步机制非账本数据",
  },
];

export interface ExemptFile {
  file: string;
  why: string;
}

export const EXEMPT_FILES: readonly ExemptFile[] = [
  {
    file: "db/migrate.rs",
    why: "迁移链本体：schema 管理是迁移职责（ADR-0111 决策 5「迁移文件免扫」）",
  },
  {
    file: "db/schema_guard.rs",
    why: "schema 漂移守卫本体：启动期 schema 校验职责（ADR-0100 / ADR-0111 决策 5）",
  },
];

export interface RegisteredException {
  file: string;
  count: number;
  why: string;
}

export const REGISTERED_EXCEPTIONS: readonly RegisteredException[] = [];

const CREATE_TABLE_RE =
  /\bCREATE\s+(?:VIRTUAL\s+)?TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?(?:["'`]?\w+["'`]?\s*\.\s*)?["'`]?(\w+)/gi;

export function deriveBannedTables(srcTauriDir: string): { inventory: string[]; banned: string[] } {
  const sources: string[] = [];
  const migrationsDir = join(srcTauriDir, "migrations");
  if (existsSync(migrationsDir)) {
    for (const entry of readDirEntries(migrationsDir)) {
      if (entry.name.endsWith(".sql"))
        sources.push(readFileSync(join(migrationsDir, entry.name), "utf8"));
    }
  }
  const migrateRs = join(srcTauriDir, INFRA_SRC_REL, "db", "migrate.rs");
  if (existsSync(migrateRs)) sources.push(readFileSync(migrateRs, "utf8"));

  const inventory = new Set<string>();
  for (const source of sources) {
    for (const m of source.matchAll(CREATE_TABLE_RE)) inventory.add(m[1]);
  }
  if (inventory.size === 0) {
    fail(
      `迁移链提不出任何 CREATE TABLE 表名（${migrationsDir} 无 .sql 或清单漂移）` +
        "——拒绝以空集假绿通过",
    );
  }

  const missing = MECHANISM_TABLES.filter((t) => !inventory.has(t.table));
  if (missing.length > 0) {
    fail(
      `机制表清单漂移：${missing.map((t) => t.table).join("、")} 不在迁移链 CREATE TABLE 清单中` +
        "——表已改名/删除，请同步 MECHANISM_TABLES（清单漂移 fail loud）",
    );
  }

  const mechanismNames = new Set(MECHANISM_TABLES.map((t) => t.table));
  const banned = [...inventory].filter((t) => !mechanismNames.has(t)).sort();
  if (banned.length === 0) {
    fail("机制表清单覆盖了迁移链全部表——禁 DML 账本数据表集合为空，拒绝以空集假绿通过");
  }
  return { inventory: [...inventory].sort(), banned };
}

export interface DmlHit {
  line: number;
  text: string;
  match: string;
}

export function dmlPattern(bannedTables: readonly string[]): RegExp {
  return new RegExp(
    "\\b(?:INSERT\\s+(?:OR\\s+\\w+\\s+)?INTO|REPLACE\\s+INTO|UPDATE|DELETE\\s+FROM)\\s+" +
      "(?:[\"'`]?\\w+[\"'`]?\\s*\\.\\s*)?[\"'`]?" +
      `(?:${bannedTables.join("|")})\\b`,
    "gi",
  );
}

export function scanDml(masked: string, bannedTables: readonly string[]): DmlHit[] {
  const rawLines = masked.split("\n");
  const hits: DmlHit[] = [];
  const re = dmlPattern(bannedTables);
  for (const m of masked.matchAll(re)) {
    const line = lineAt(masked, m.index);
    hits.push({ line, text: (rawLines[line - 1] ?? "").trim(), match: m[0] });
  }
  return hits;
}

function isTestPath(relSegments: string[]): boolean {
  return (
    relSegments.slice(0, -1).includes("tests") || relSegments[relSegments.length - 1] === "tests.rs"
  );
}

function main(): void {
  const srcTauri = process.argv[2] ? resolve(process.argv[2]) : DEFAULT_SRC_TAURI;
  const infraSrcDir = join(srcTauri, INFRA_SRC_REL);
  if (!existsSync(infraSrcDir)) fail(`基础设施 crate 源目录不存在：${infraSrcDir}`);

  const { inventory, banned } = deriveBannedTables(srcTauri);

  const problems: string[] = [];
  for (const e of EXEMPT_FILES) {
    if (!existsSync(join(infraSrcDir, e.file))) {
      problems.push(
        `✗ 免扫清单漂移：${INFRA_SRC_REL}/${e.file} 不存在（${e.why}）——` +
          "文件改名/搬迁后未同步 EXEMPT_FILES，请复核后更新登记",
      );
    }
  }

  const allFiles = walkTextFiles(infraSrcDir, INFRA_SRC_REL, { extensions: RUST_EXTENSIONS });
  const scannedFiles: { abs: string; rel: string }[] = [];
  const hitsByFile = new Map<string, DmlHit[]>();
  const exemptRel = new Set(EXEMPT_FILES.map((e) => `${INFRA_SRC_REL}/${e.file}`));
  for (const f of allFiles) {
    const segments = f.rel.split("/");
    if (isTestPath(segments)) continue;
    if (exemptRel.has(f.rel)) continue;
    scannedFiles.push(f);
    const source = readFileSync(f.abs, "utf8");
    const masked = maskNonCode(source, true);
    const hits = scanDml(masked, banned);
    if (hits.length > 0) hitsByFile.set(f.rel, hits);
  }

  for (const [rel, hits] of [...hitsByFile.entries()].sort()) {
    if (REGISTERED_EXCEPTIONS.some((e) => `${INFRA_SRC_REL}/${e.file}` === rel)) continue;
    for (const hit of hits) {
      problems.push(
        `✗ 账本数据表 DML：${rel}:${hit.line}（${hit.match}）\n` +
          `    ${hit.text}\n` +
          `    基础设施不得对账本数据表执行 DML（ADR-0111 决策 1、决策 5 / issue #1135；` +
          `承接 ADR-0071 守门意图）：账本数据的语义与算术归域——\n` +
          `    写路径收敛到域层公开写入口；迁移与 schema 守卫按职责免扫` +
          `（明示登记于本脚本 EXEMPT_FILES），其余位置命中即红`,
      );
    }
  }

  for (const e of REGISTERED_EXCEPTIONS) {
    const rel = `${INFRA_SRC_REL}/${e.file}`;
    if (!scannedFiles.some((f) => f.rel === rel)) {
      problems.push(
        `✗ 例外清单漂移：${rel} 不在扫描范围（不存在或已改为测试豁免形态）——` +
          `文件改名/搬迁后未同步 REGISTERED_EXCEPTIONS（${e.why}），请复核后更新登记`,
      );
      continue;
    }
    const actual = hitsByFile.get(rel)?.length ?? 0;
    if (actual !== e.count) {
      problems.push(
        actual === 0
          ? `✗ 例外已收敛：${rel} 实际命中 0 处——登记条目应删除（失效登记是第二份事实，` +
              `请同步清理 REGISTERED_EXCEPTIONS）`
          : `✗ 例外漂移：${rel} 实际命中 ${actual} 处 ≠ 登记的 ${e.count} 处（${e.why}）——` +
              "多出的命中未登记新直置即红，复核后同步更新代码与登记",
      );
    }
  }

  if (problems.length > 0) {
    for (const p of problems) console.error(p);
    console.error(
      `❌ 基础设施账本数据表 DML 禁令失败：${problems.length} 处问题` +
        `（禁 DML 账本数据表 ${banned.length} 张，迁移链全量 ${inventory.length} 张；` +
        `ADR-0111 决策 1、决策 5 / issue #1135）`,
    );
    process.exit(1);
  }

  console.log(
    `✓ 基础设施账本数据表 DML 禁令通过：禁 DML 账本数据表 ${banned.length} 张` +
      `（迁移链全量 ${inventory.length} 张 − 机制表 ${MECHANISM_TABLES.length} 张：` +
      `${MECHANISM_TABLES.map((t) => t.table).join(" / ")}）\n` +
      `  · 免扫文件（明示理由）：${EXEMPT_FILES.map((e) => `${e.file}（${e.why}）`).join("；")}\n` +
      `  · 已登记例外 ${REGISTERED_EXCEPTIONS.length} 条（严格相等校验）：` +
      `${REGISTERED_EXCEPTIONS.length > 0 ? REGISTERED_EXCEPTIONS.map((e) => e.file).join("、") : "无——#1108 起壳机制迁出根包，例外随迁退役"}\n` +
      `  · 扫描 ${INFRA_SRC_REL} 非测试文件 ${scannedFiles.length} 个零生产命中（issue #1135 / ADR-0111）`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
