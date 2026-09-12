#!/usr/bin/env bun
// 基础设施账本数据表 DML 禁令（issue #1135 / ADR-0111 决策 1、决策 5；承接
// ADR-0071「余额与净资产口径迁出基础设施」的守门意图）：
// 基础设施（ledger-infra crate）只承诺「不定义账本数据的口径与规则」——交易、
// 账户余额、净资产、预算、投资等账本数据的语义与算术一律归域，基础设施不得对
// 账本数据表执行 DML（INSERT / UPDATE / DELETE）。
//
// 账本数据表判定（默认拒绝，防新表绕过）：表清单从迁移链提取（migrations/*.sql
// 与 db/migrate.rs 的 CREATE TABLE 全量），除「机制表」（MECHANISM_TABLES，逐条
// 附理由显式登记——设置 KV 归口与多端同步协议表，非账本数据）外全部视为账本
// 数据表。迁移链新表进场未登记机制表即按账本数据对待，基础设施对它执行 DML
// 必然变红——新表的归类被迫显式化，不靠沉默放行。
//
// 免扫范围（明示理由，不靠沉默放行；ADR-0111 决策 5「迁移与 schema 守卫免扫」）：
// - 迁移文件本体（migrations/*.sql，不在扫描根）与迁移链 db/migrate.rs——
//   schema 管理是迁移职责；
// - schema 守卫 db/schema_guard.rs——启动期 schema 漂移校验职责（ADR-0100）。
// 免扫条目 fail loud：文件消失/改名即红（清单漂移），不放任守门空转。
//
// 测试豁免（ADR-0056 决策 5，与守门家族同款）：外挂测试模块/目录（tests.rs
// 文件、tests/ 目录）整文件豁免——测试侧业务表直置归公开写入口纪律辖域
// （check-test-support.ts）。内联 #[cfg(test)] 块不豁免（与家族一致，更严）：
// 存量内联夹具的命中经 REGISTERED_EXCEPTIONS 逐条登记（文件 + 预期命中数 +
// 动机），实际命中与登记数严格相等校验——多出（未登记新直置）、少掉（例外已
// 收敛）都红，与「例外显式登记在案，防止被无意复制」同哲学（ADR-0073 例外
// 白名单纪律）。
//
// 扫描边界（文本级）：SQL 就住在字符串字面量里，掩码只抹注释、保留字符串
// （maskNonCode(…, true)，与 check-structure.ts 原生事务语句禁令同款）；
// 注释里的形态不计数。命中形态覆盖 `INSERT [OR x] INTO` / `REPLACE INTO` /
// `UPDATE` / `DELETE FROM`，含 `main.` 库名前缀限定（`main.transactions` 同样
// 命中）。已知文本不可达逃逸形态（靠评审兜底）：经变量拼接的动态表名、
// `UPDATE … SET … FROM <表>` 的 FROM 侧（只读不属 DML 靶）、宏拼接的语句形态。
// 扫描根 = `crates/infra/src`（INFRA_SRC_REL 单一来源），拆 crate 后路径随清单走。
//
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-infra-dml.ts`。
// 默认校验本仓库；测试可传位置参数指向夹具：bun scripts/check-infra-dml.ts [src-tauri-dir]
// 挂载于 scripts/check.sh 质量门槛序列；包装测试 src/__tests__/check-infra-dml.test.ts。

import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { INFRA_SRC_REL, maskNonCode } from './check-structure.ts'

const DEFAULT_SRC_TAURI = join(fileURLToPath(import.meta.url), '..', '..', 'src-tauri')

function fail(message: string): never {
  console.error(`✗ 基础设施账本数据表 DML 禁令：${message}`)
  process.exit(1)
}

/** 机制表条目：非账本数据的基础设施机制表，逐条附理由（显式登记，不靠沉默放行） */
export interface MechanismTable {
  table: string
  why: string
}

/**
 * 机制表（非账本数据）：迁移链全量表清单扣除本清单即禁 DML 账本数据表集合
 * （默认拒绝——新表未登记机制表即按账本数据对待）。每条附理由留痕。
 */
export const MECHANISM_TABLES: readonly MechanismTable[] = [
  {
    table: 'app_settings',
    why: '设置 KV 归口（crate::settings 单点收口，ADR-0111 共享接缝）——设置非账本数据',
  },
  {
    table: 'sync_device',
    why: '多端同步协议表（协议 crate 唯一 SQL 收口，#1089 / ADR-0091）——同步机制非账本数据',
  },
  {
    table: 'sync_ops',
    why: '多端同步协议表（协议 crate 唯一 SQL 收口，#1089 / ADR-0091）——同步机制非账本数据',
  },
  {
    table: 'sync_parked_ops',
    why: '多端同步协议表（协议 crate 唯一 SQL 收口，#1089 / ADR-0091）——同步机制非账本数据',
  },
  {
    table: 'sync_stream_positions',
    why: '多端同步协议表（协议 crate 唯一 SQL 收口，#1089 / issue #857）——同步机制非账本数据',
  },
]

/** 免扫文件条目：按职责免扫，逐条附理由（明示，不靠沉默放行） */
export interface ExemptFile {
  file: string
  why: string
}

/**
 * 免扫文件（相对 `crates/infra/src`，ADR-0111 决策 5）：迁移与 schema 守卫按其
 * 职责免扫。fail loud：登记文件消失即红（清单漂移），不放任守门空转。
 */
export const EXEMPT_FILES: readonly ExemptFile[] = [
  {
    file: 'db/migrate.rs',
    why: '迁移链本体：schema 管理是迁移职责（ADR-0111 决策 5「迁移文件免扫」）',
  },
  {
    file: 'db/schema_guard.rs',
    why: 'schema 漂移守卫本体：启动期 schema 校验职责（ADR-0100 / ADR-0111 决策 5）',
  },
]

/** 已登记例外条目：内联 cfg(test) 测试夹具的存量账本表 DML 命中 */
export interface RegisteredException {
  file: string
  count: number
  why: string
}

/**
 * 已登记例外（严格相等校验）：内联 #[cfg(test)] 不豁免（家族一致），存量内联
 * 夹具命中逐条登记——实际命中 ≠ 登记数即红（漂移），命中清零也红（例外已
 * 收敛，登记条目应删除）；登记文件消失（改名/搬迁）同样红（清单漂移，与
 * EXEMPT_FILES 同纪律）。
 */
export const REGISTERED_EXCEPTIONS: readonly RegisteredException[] = [
  {
    file: 'shell_support/write_entry.rs',
    count: 1,
    why: '内联 cfg(test) 测试夹具：验证写入口闭包拿到可用连接（写入 categories 落库）——测试侧业务表直置归公开写入口纪律辖域，此处登记防命中数漂移；路径随 #1130 壳机制分组（shell_support/）迁移更新',
  },
]

// 迁移链 CREATE TABLE 提取形态（含 VIRTUAL / IF NOT EXISTS / 引号包裹 /
// 库名前缀限定——`CREATE TABLE main.foo` 取末段标识符 foo 入清单）
const CREATE_TABLE_RE =
  /\bCREATE\s+(?:VIRTUAL\s+)?TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?(?:["'`]?\w+["'`]?\s*\.\s*)?["'`]?(\w+)/gi

/**
 * 账本数据表判定单一来源：从迁移链（migrations/*.sql + 迁移链定义
 * db/migrate.rs）提取 CREATE TABLE 全量清单，扣除机制表即禁 DML 集合。
 * 提不出任何表名即红（拒绝以空集假绿通过）。
 */
export function deriveBannedTables(srcTauriDir: string): { inventory: string[]; banned: string[] } {
  const sources: string[] = []
  const migrationsDir = join(srcTauriDir, 'migrations')
  if (existsSync(migrationsDir)) {
    for (const entry of readdirSync(migrationsDir).sort()) {
      if (entry.endsWith('.sql')) sources.push(readFileSync(join(migrationsDir, entry), 'utf8'))
    }
  }
  const migrateRs = join(srcTauriDir, INFRA_SRC_REL, 'db', 'migrate.rs')
  if (existsSync(migrateRs)) sources.push(readFileSync(migrateRs, 'utf8'))

  const inventory = new Set<string>()
  for (const source of sources) {
    for (const m of source.matchAll(CREATE_TABLE_RE)) inventory.add(m[1])
  }
  if (inventory.size === 0) {
    fail(
      `迁移链提不出任何 CREATE TABLE 表名（${migrationsDir} 无 .sql 或清单漂移）` +
        '——拒绝以空集假绿通过',
    )
  }

  const missing = MECHANISM_TABLES.filter((t) => !inventory.has(t.table))
  if (missing.length > 0) {
    fail(
      `机制表清单漂移：${missing.map((t) => t.table).join('、')} 不在迁移链 CREATE TABLE 清单中` +
        '——表已改名/删除，请同步 MECHANISM_TABLES（清单漂移 fail loud）',
    )
  }

  const mechanismNames = new Set(MECHANISM_TABLES.map((t) => t.table))
  const banned = [...inventory].filter((t) => !mechanismNames.has(t)).sort()
  if (banned.length === 0) {
    fail('机制表清单覆盖了迁移链全部表——禁 DML 账本数据表集合为空，拒绝以空集假绿通过')
  }
  return { inventory: [...inventory].sort(), banned }
}

/** 单条 DML 命中：行号（1 起算）、原文行、匹配文本 */
export interface DmlHit {
  line: number
  text: string
  match: string
}

/**
 * DML 禁令形态（针对禁 DML 表集合，大小写不敏感）：
 * `INSERT [OR x] INTO <table>` / `REPLACE INTO <table>` / `UPDATE <table>` /
 * `DELETE FROM <table>`，含 `main.` 库名前缀限定（可选 schema 段跳过后取表名）。
 * SQL 就住在字符串里，调用方须以 keepLiterals=true 掩码（只抹注释）。
 */
export function dmlPattern(bannedTables: readonly string[]): RegExp {
  return new RegExp(
    '\\b(?:INSERT\\s+(?:OR\\s+\\w+\\s+)?INTO|REPLACE\\s+INTO|UPDATE|DELETE\\s+FROM)\\s+' +
      '(?:["\'`]?\\w+["\'`]?\\s*\\.\\s*)?["\'`]?' +
      `(?:${bannedTables.join('|')})\\b`,
    'gi',
  )
}

/** 扫描单个 Rust 源文本（调用方传入 keepLiterals 掩码后的文本）：返回全部命中 */
export function scanDml(masked: string, bannedTables: readonly string[]): DmlHit[] {
  const rawLines = masked.split('\n')
  const hits: DmlHit[] = []
  const re = dmlPattern(bannedTables)
  for (const m of masked.matchAll(re)) {
    const line = (masked.slice(0, m.index ?? 0).match(/\n/g)?.length ?? 0) + 1
    hits.push({ line, text: (rawLines[line - 1] ?? '').trim(), match: m[0] })
  }
  return hits
}

/** 测试豁免形态（ADR-0056 决策 5，与守门家族同款）：tests.rs 文件与 tests/ 目录 */
function isTestPath(relSegments: string[]): boolean {
  return relSegments.slice(0, -1).includes('tests') || relSegments[relSegments.length - 1] === 'tests.rs'
}

/** 递归收集目录下全部 .rs 文件（相对路径排序保证输出确定） */
function walkRustFiles(dir: string, relBase: string): { abs: string; rel: string }[] {
  const out: { abs: string; rel: string }[] = []
  for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name),
  )) {
    const abs = join(dir, entry.name)
    const rel = relBase ? `${relBase}/${entry.name}` : entry.name
    if (entry.isDirectory()) out.push(...walkRustFiles(abs, rel))
    else if (entry.name.endsWith('.rs')) out.push({ abs, rel })
  }
  return out
}

function main(): void {
  const srcTauri = process.argv[2] ? resolve(process.argv[2]) : DEFAULT_SRC_TAURI
  const infraSrcDir = join(srcTauri, INFRA_SRC_REL)
  if (!existsSync(infraSrcDir)) fail(`基础设施 crate 源目录不存在：${infraSrcDir}`)

  const { inventory, banned } = deriveBannedTables(srcTauri)

  // 免扫清单自证：登记文件必须存在（清单漂移 fail loud，不放任守门空转）
  const problems: string[] = []
  for (const e of EXEMPT_FILES) {
    if (!existsSync(join(infraSrcDir, e.file))) {
      problems.push(
        `✗ 免扫清单漂移：${INFRA_SRC_REL}/${e.file} 不存在（${e.why}）——` +
          '文件改名/搬迁后未同步 EXEMPT_FILES，请复核后更新登记',
      )
    }
  }

  // 全量收集 → 免扫/外挂测试分流 → 生产文本扫描（掩码只抹注释、保留 SQL 字符串）
  const allFiles = walkRustFiles(infraSrcDir, INFRA_SRC_REL)
  const scannedFiles: { abs: string; rel: string }[] = []
  const hitsByFile = new Map<string, DmlHit[]>()
  const exemptRel = new Set(EXEMPT_FILES.map((e) => `${INFRA_SRC_REL}/${e.file}`))
  for (const f of allFiles) {
    const segments = f.rel.split('/')
    if (isTestPath(segments)) continue // 外挂测试整文件豁免（ADR-0056 决策 5）
    if (exemptRel.has(f.rel)) continue // 免扫职责（明示登记于 EXEMPT_FILES）
    scannedFiles.push(f)
    const source = readFileSync(f.abs, 'utf8')
    const masked = maskNonCode(source, true)
    const hits = scanDml(masked, banned)
    if (hits.length > 0) hitsByFile.set(f.rel, hits)
  }

  // 生产命中即红（纯禁令，定位到 文件:行）；已登记例外文件的命中数由下方严格
  // 相等校验裁决（漂移/收敛才红），不在此逐条报。
  for (const [rel, hits] of [...hitsByFile.entries()].sort()) {
    if (REGISTERED_EXCEPTIONS.some((e) => `${INFRA_SRC_REL}/${e.file}` === rel)) continue
    for (const hit of hits) {
      problems.push(
        `✗ 账本数据表 DML：${rel}:${hit.line}（${hit.match}）\n` +
          `    ${hit.text}\n` +
          `    基础设施不得对账本数据表执行 DML（ADR-0111 决策 1、决策 5 / issue #1135；` +
          `承接 ADR-0071 守门意图）：账本数据的语义与算术归域——\n` +
          `    写路径收敛到域层公开写入口；迁移与 schema 守卫按职责免扫` +
          `（明示登记于本脚本 EXEMPT_FILES），其余位置命中即红`,
      )
    }
  }

  // 已登记例外严格相等校验：漂移（≠登记数）即红、收敛（清零）即红；
  // 登记文件消失（改名/搬迁）也红（清单漂移 fail loud，与 EXEMPT_FILES 同纪律）。
  for (const e of REGISTERED_EXCEPTIONS) {
    const rel = `${INFRA_SRC_REL}/${e.file}`
    if (!scannedFiles.some((f) => f.rel === rel)) {
      problems.push(
        `✗ 例外清单漂移：${rel} 不在扫描范围（不存在或已改为测试豁免形态）——` +
          `文件改名/搬迁后未同步 REGISTERED_EXCEPTIONS（${e.why}），请复核后更新登记`,
      )
      continue
    }
    const actual = hitsByFile.get(rel)?.length ?? 0
    if (actual !== e.count) {
      problems.push(
        actual === 0
          ? `✗ 例外已收敛：${rel} 实际命中 0 处——登记条目应删除（失效登记是第二份事实，` +
              `请同步清理 REGISTERED_EXCEPTIONS）`
          : `✗ 例外漂移：${rel} 实际命中 ${actual} 处 ≠ 登记的 ${e.count} 处（${e.why}）——` +
              '多出的命中未登记新直置即红，复核后同步更新代码与登记',
      )
    }
  }

  if (problems.length > 0) {
    for (const p of problems) console.error(p)
    console.error(
      `❌ 基础设施账本数据表 DML 禁令失败：${problems.length} 处问题` +
        `（禁 DML 账本数据表 ${banned.length} 张，迁移链全量 ${inventory.length} 张；` +
        `ADR-0111 决策 1、决策 5 / issue #1135）`,
    )
    process.exit(1)
  }

  console.log(
    `✓ 基础设施账本数据表 DML 禁令通过：禁 DML 账本数据表 ${banned.length} 张` +
      `（迁移链全量 ${inventory.length} 张 − 机制表 ${MECHANISM_TABLES.length} 张：` +
      `${MECHANISM_TABLES.map((t) => t.table).join(' / ')}）\n` +
      `  · 免扫文件（明示理由）：${EXEMPT_FILES.map((e) => `${e.file}（${e.why}）`).join('；')}\n` +
      `  · 已登记例外 ${REGISTERED_EXCEPTIONS.length} 条（严格相等校验）：` +
      `${REGISTERED_EXCEPTIONS.map((e) => e.file).join('、')}\n` +
      `  · 扫描 ${INFRA_SRC_REL} 非测试文件 ${scannedFiles.length} 个零生产命中（issue #1135 / ADR-0111）`,
  )
}

// 仅直接运行时执行 main；包装测试以子进程调用（行为等价判据：只测外部可观察结果）。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
