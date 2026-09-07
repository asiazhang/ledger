#!/usr/bin/env bun
// Rust 测试守门（issue #752 / ADR-0084 决策 8），三条规则：
//
// 统一测试数据库工厂（src-tauri/src/test_support/，#751 落地）是建库与种子知识的
// 唯一入口；本守门防回潮——测试代码绕开工厂直连建库、直写种子表、自抄默认时刻，
// 一律红。白名单起步、覆盖全部存量（「白名单即规格」，ADR-0056 哲学），随按域
// 迁移票逐组缩减为空（API 集成组已随 #753 清零，叶子域+db+sync 组已随 #754
// 清零），#758 收口转纯禁令。前端同构先例：
// check-test-stubs.ts（#725/#726）已验证「深模块收敛 + 文本级守门 + 白名单防回潮」
// 在本仓有效。
//
// 规则 1（禁直连建库）：`open_in_memory` / `init_db` 出现在 test_support 与
// src/db 产品代码之外的 Rust 测试代码即红（建库两行序的唯一入口是
// test_support::open()）。src/db/tests 同样在禁用范围——产品代码合法、测试不豁免；
// src/db 产品代码内的命中不在扫描范围（产品开库合法）。匹配按裸标识符：任意
// 限定路径（db::/crate::db::/tauri_app_lib::db:: 等）、裸调用与 use 引入、以及
// 测试侧平行建库入口（如 `DbState::open_in_memory`——内部即两行序）与方法形态
// 全部命中；`init_db`/`open_in_memory` 作为更长标识符的子串不匹配（\b 边界）。
//
// 规则 2（禁夹具裸 SQL）：工厂种子表的 `INSERT INTO` 出现在 test_support 与
// 各域薄皮之外即红。禁用表集合单一来源 = test_support/seed.rs 的 `INSERT INTO`
// 登记处（提取其表名；工厂未来新种子自动扩展禁令，与 check-test-stubs.ts 从
// REFERENCE_DEFAULTS 提取命令清单同款，无双源漂移）；登记处提不出任何表名即红。
// 薄皮豁免按文件名形状：父目录恰为 tests 目录段且文件名为 common.rs /
// batch_common.rs——域薄皮按准入规则长期保留单域特有种子（ADR-0084 决策 1）。
// 顶屋 tests/ 下的子目录共享层（tests/api_server/common.rs、tests/e2e/common.rs）
// 不豁免——它们的种子表 INSERT 与建库命中照常计入/入白名单，可见才可减。
// 业务表（transactions/categories/budgets 等）的 INSERT 不在本守门范围——那是
// 公开写入口纪律（测试层显式例外逐个登记）的辖域，不是建库/种子工厂的。
//
// 规则 3（禁默认时刻字面量）：`2026-01-01T00:00:00Z`（FIXED_NOW 现值）出现在
// test_support 之外的测试代码即红——夹具簿记戳由工厂种子内部发放，调用点零字面量。
// 域时刻字面量合法（时间推进是测试的行为输入，ADR-0084 决策 5），不在扫描范围；
// 文本级无法分辨意图，恰为该值的字面量一律计入（如实际作域时刻用，迁移票缩减
// 白名单时改写为其他值或引用常量）。
//
// 扫描边界（文本级，注释掩码后匹配，注释里的形态不计数）：
// - 「测试代码」= ① 路径含 tests 目录段或文件名为 tests.rs 的文件（域外挂测试
//   模块/目录与顶层集成测试，check-structure.ts 同款约定）整文件；
//   ② 其余产品文件内的 `#[cfg(test)] mod <name> { … }` 内联块（括号配对，词法
//   跳过字符串与注释）。产品代码本体不扫——产品开库、产品写表、产品默认时刻
//   均合法，三条规则只辖测试代码。
// - 字符串字面量不掩码（SQL 就住在字符串里）；注释（行/块，含嵌套块注释）掩码
//   为等长空白。裸字符串 "…" 处理转义，r"…" / r#"…"# 等原始字符串按 hash 数配对
//   终止；'…' 仅按合法 char 字面量吞掉，其余（生命周期 'a、标签 'outer:）跳过
//   单字符。
//
// 已知文本不可达逃逸形态（靠评审兜底）：经 use 别名（use … as x; 调 x()）或局部
// 函数包装的间接建库/写表；宏拼接出的命中形态；`db :: f`（空格分隔限定符）；
// 属性内部含不平衡括号的非常规写法。char 字面量形态枚举之外的怪异转义可能致
// 词法错位——失衡区段跳过不计（宁漏不误）。
//
// 白名单语义（下方 WHITELIST）：每条目 = 一个文件在各规则的允许命中数，注释记
// 行数作迁移票递减基线。命中数与声明数**必须严格相等**：超出红（违规，收敛到
// 工厂/薄皮），低于也红（基线过期——迁移票必须同步缩减声明数，白名单即规格）。
// 条目文件消失或全字段清零即红（清单漂移 fail loud）。
//
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-test-support.ts`。
// 默认校验本仓库（应用白名单）；测试可传位置参数指向夹具（夹具不应用白名单，
// 全部命中按未入白名单报告）：bun scripts/check-test-support.ts [src-tauri-dir]
// 挂载于 scripts/check.sh 质量门槛序列；包装测试 src/__tests__/check-test-support.test.ts。

import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { pathToFileURL } from 'node:url'

const DEFAULT_SRC_TAURI = join(fileURLToPath(import.meta.url), '..', '..', 'src-tauri')

/** 白名单条目：一个文件在各规则的允许命中数（严格相等，注释记行数作递减基线） */
export interface GateWhitelistEntry {
  /** 文件路径，相对 src-tauri，'/' 分隔 */
  file: string
  /** 域分组说明（迁移票按组认领） */
  note: string
  /** 规则 1（禁直连建库）允许命中数；缺省 = 0 */
  r1?: number
  /** 规则 2（禁夹具裸 SQL）允许命中数；缺省 = 0 */
  r2?: number
  /** 规则 3（禁默认时刻字面量）允许命中数；缺省 = 0 */
  r3?: number
}

/**
 * 白名单起步（issue #752）：现存全部违规按域分组，一组一行、注释记行数。
 * 每张按域迁移票负责把自己那组缩减为空（#757 transaction；e2e 与 backup/内联簿记
 * 组暂无对应迁移票，随 e2e 夹具 spec 或对应域票处置）。API 集成层组已随 #753 清零，
 * 叶子域+db+sync 组已随 #754 清零，investment 组已随 #755 清零，scheduled_transactions
 * 组已随 #756 清零。#758 收口：全表清零转纯禁令。
 */
export const WHITELIST: readonly GateWhitelistEntry[] = [

  // ── transaction 域（迁移票 #757）──
  { file: 'src/transaction/tests/amount.rs', note: 'transaction 域', r1: 2, r2: 2, r3: 2 },
  { file: 'src/transaction/tests/audit.rs', note: 'transaction 域', r2: 1 },
  { file: 'src/transaction/tests/batch_common.rs', note: 'transaction 域', r1: 4, r3: 2 },
  { file: 'src/transaction/tests/batch_create.rs', note: 'transaction 域', r2: 4, r3: 10 },
  { file: 'src/transaction/tests/behavior.rs', note: 'transaction 域', r3: 4 },
  { file: 'src/transaction/tests/category.rs', note: 'transaction 域', r3: 2 },
  { file: 'src/transaction/tests/common.rs', note: 'transaction 域', r1: 4, r3: 8 },
  { file: 'src/transaction/tests/merchant.rs', note: 'transaction 域', r3: 2 },
  { file: 'src/transaction/tests/query.rs', note: 'transaction 域', r3: 1 },
  { file: 'src/transaction/tests/search.rs', note: 'transaction 域', r1: 4, r2: 1, r3: 20 },
  { file: 'src/transaction/tests/search_repair.rs', note: 'transaction 域', r1: 4, r2: 1, r3: 4 },
  { file: 'src/transaction/writer/tests/common.rs', note: 'transaction 域', r1: 2, r3: 4 },
  { file: 'src/transaction/writer/tests/merchant.rs', note: 'transaction 域', r3: 2 },
  { file: 'src/transaction/writer/tests/normalize.rs', note: 'transaction 域', r2: 1 },
  { file: 'src/transaction/writer/tests/rows.rs', note: 'transaction 域', r1: 1 },

  // ── e2e（夹具统一另立 spec，暂无迁移票）──
  { file: 'tests/e2e/accounts_steps.rs', note: 'e2e', r2: 2, r3: 1 },
  { file: 'tests/e2e/backup_steps.rs', note: 'e2e', r1: 3, r2: 1 },
  { file: 'tests/e2e/common.rs', note: 'e2e', r2: 1, r3: 2 },
  { file: 'tests/e2e/world.rs', note: 'e2e', r1: 1 },
  { file: 'tests/e2e/dashboard_steps.rs', note: 'e2e', r2: 1 },
  { file: 'tests/e2e/data_location_steps.rs', note: 'e2e', r1: 4 },
  { file: 'tests/e2e/encryption_steps.rs', note: 'e2e', r1: 3, r2: 1, r3: 2 },
  { file: 'tests/e2e/financial_freedom_steps.rs', note: 'e2e', r2: 1 },
  { file: 'tests/e2e/fund_trade_steps.rs', note: 'e2e', r2: 1 },
  { file: 'tests/e2e/instruments_steps.rs', note: 'e2e', r2: 3 },
  { file: 'tests/e2e/investment_trend_steps.rs', note: 'e2e', r2: 2 },
  { file: 'tests/e2e/scheduled_steps/occurrence.rs', note: 'e2e', r2: 1 },
  { file: 'tests/e2e/startup_failure_steps.rs', note: 'e2e', r1: 2, r2: 1, r3: 2 },
  { file: 'tests/e2e/transactions_query_steps.rs', note: 'e2e', r3: 4 },

  // ── backup 域与壳/基础设施内联 cfg(test)（暂无迁移票）──
  { file: 'src/backup/auto.rs', note: 'backup/内联簿记', r1: 5 },
  { file: 'src/backup/tests.rs', note: 'backup/内联簿记', r1: 13, r2: 1, r3: 2 },
  { file: 'src/logger.rs', note: 'backup/内联簿记', r1: 2 },
  { file: 'src/settings.rs', note: 'backup/内联簿记', r1: 6 },
  { file: 'src/write_entry.rs', note: 'backup/内联簿记', r1: 1, r3: 2 },
]

function fail(message: string): never {
  console.error(`✗ Rust 测试守门：${message}`)
  process.exit(1)
}

// ——— Rust 词法助手（掩码与 cfg(test) 块提取共享） ———

const CHAR_LITERAL_RE = /^'(\\(?:.|x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]{1,6}\})|[^\\'])'/

/** 若 source[i] 起是字符串字面量起点（含 b/r/br 前缀与原始字符串 r#"…"#），返回终点下标；否则 -1 */
function stringEndAt(source: string, i: number): number {
  const isRaw = source[i] === 'r' && (source[i + 1] === '"' || source[i + 1] === '#')
  const isByteRaw = source[i] === 'b' && source[i + 1] === 'r' && (source[i + 2] === '"' || source[i + 2] === '#')
  if (source[i] !== '"' && source[i] !== 'b' && !isRaw && !isByteRaw) return -1
  let j = i
  let rawHashes = 0
  if (source[j] === 'b') j++
  if (source[j] === 'r') {
    j++
    while (source[j] === '#') { rawHashes++; j++ }
  }
  if (source[j] !== '"') return -1 // 形态不符（如裸 r 后接标识符）
  j++
  const terminator = '"' + '#'.repeat(rawHashes)
  while (j < source.length) {
    if (rawHashes === 0 && source[j] === '\\') { j += 2; continue }
    if (source.startsWith(terminator, j)) return j + terminator.length
    j++
  }
  return source.length // 未闭合：吞到文件尾（宁漏不误）
}

/** 若 source[i] 起是 char 字面量，返回终点下标；否则 -1（生命周期 'a、标签 'outer: 等情形，
 *  调用方自行跳过单字符）。 */
function charEndAt(source: string, i: number): number {
  const m = CHAR_LITERAL_RE.exec(source.slice(i, i + 12))
  return m ? i + m[0].length : -1
}

// ——— 词法掩码：注释（行/块，嵌套）替换为等长空白；字符串/char 原样保留 ———
// 单次顺序扫描：在同一位置先判注释起点再判字符串起点，字符串内的 // 不会误开
// 注释；原始字符串按 hash 数配对；'…' 仅按合法 char 字面量吞掉，否则按生命周期
// 跳过单字符。扫描在注释起点处消耗到注释终点，注释体内一切字符不再触发词法。
function maskComments(source: string): string {
  const out = source.split('')
  let i = 0
  const blank = (from: number, to: number): void => {
    for (let k = from; k < to && k < out.length; k++) if (out[k] !== '\n') out[k] = ' '
  }
  while (i < source.length) {
    const c = source[i]
    if (c === '/' && source[i + 1] === '/') {
      let e = i
      while (e < source.length && source[e] !== '\n') e++
      blank(i, e)
      i = e
    } else if (c === '/' && source[i + 1] === '*') {
      let depth = 1
      let e = i + 2
      while (e < source.length && depth > 0) {
        if (source[e] === '/' && source[e + 1] === '*') { depth++; e += 2 }
        else if (source[e] === '*' && source[e + 1] === '/') { depth--; e += 2 }
        else e++
      }
      blank(i, e)
      i = e
    } else {
      const strEnd = stringEndAt(source, i)
      if (strEnd !== -1) { i = strEnd; continue }
      if (c === "'") { i = Math.max(charEndAt(source, i), i + 1); continue }
      i++
    }
  }
  return out.join('')
}

// ——— cfg(test) 内联块提取：属性后随 `mod <name> {`，括号配对取块范围 ———
interface Region { start: number; end: number }

function extractCfgTestRegions(masked: string): Region[] {
  const regions: Region[] = []
  let i = 0
  while ((i = masked.indexOf('#[cfg(test)]', i)) !== -1) {
    i += '#[cfg(test)]'.length
    // 跳过后随的其余属性（#[allow(...)] 等）与空白
    for (;;) {
      while (i < masked.length && /\s/.test(masked[i])) i++
      if (masked[i] === '#') {
        const close = masked.indexOf(']', i)
        if (close === -1) break
        i = close + 1
      } else break
    }
    const mod = /^mod\s+\w+\s*\{/.exec(masked.slice(i, i + 64))
    if (!mod) continue // 外挂 `mod x;` 或非 mod 项（use/fn）：不属于本扫描的内联块
    const bodyStart = i + mod[0].length
    let depth = 1
    let j = bodyStart
    while (j < masked.length && depth > 0) {
      const strEnd = stringEndAt(masked, j)
      if (strEnd !== -1) { j = strEnd; continue }
      if (masked[j] === "'") { j = Math.max(charEndAt(masked, j), j + 1); continue }
      const c = masked[j]
      if (c === '{') depth++
      else if (c === '}') depth--
      j++
    }
    if (depth !== 0) continue // 配对失衡：跳过该块，宁漏不误
    regions.push({ start: i, end: j })
  }
  return regions
}

// ——— 三条规则的命中形态 ———
// 规则 1：裸标识符匹配（任意限定路径、裸调用与 use 引入、测试侧平行建库入口
// 如 DbState::open_in_memory 与方法形态全部命中；\b 边界排除更长标识符的子串）。
const RULE1_IDENTIFIERS = /\b(open_in_memory|init_db)\b/g
// 规则 3：FIXED_NOW 现值（字面量形态，含于字符串内）。
const RULE3_LITERAL = '2026-01-01T00:00:00Z'

interface Hit { rule: 1 | 2 | 3; line: number }

function findHits(masked: string, lineOf: (offset: number) => number, bannedTables: string[]): Hit[] {
  const hits: Hit[] = []
  RULE1_IDENTIFIERS.lastIndex = 0
  let m: RegExpExecArray | null
  while ((m = RULE1_IDENTIFIERS.exec(masked))) hits.push({ rule: 1, line: lineOf(m.index) })
  if (bannedTables.length > 0) {
    const re2 = new RegExp(`INSERT\\s+INTO\\s+(?:${bannedTables.join('|')})\\b`, 'gi')
    let m2: RegExpExecArray | null
    while ((m2 = re2.exec(masked))) hits.push({ rule: 2, line: lineOf(m2.index) })
  }
  let at = masked.indexOf(RULE3_LITERAL)
  while (at !== -1) {
    hits.push({ rule: 3, line: lineOf(at) })
    at = masked.indexOf(RULE3_LITERAL, at + RULE3_LITERAL.length)
  }
  return hits
}

// ——— 文件收集：src/** 与 tests/** 下全部 .rs ———
function walkRustFiles(dir: string): string[] {
  const out: string[] = []
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, entry.name)
    if (entry.isDirectory()) out.push(...walkRustFiles(p))
    else if (entry.name.endsWith('.rs')) out.push(p)
  }
  return out
}

/** 路径含 tests 目录段，或文件名为 tests.rs（外挂测试模块/目录约定，check-structure.ts 同款） */
function isTestPath(relSegments: string[]): boolean {
  return relSegments.slice(0, -1).includes('tests') || relSegments[relSegments.length - 1] === 'tests.rs'
}

/** 薄皮：父目录恰为 tests 目录段且文件名为 common.rs / batch_common.rs（域薄皮）；
 *  顶屋 tests/ 子目录下的共享层（tests/api_server/common.rs 等）不豁免，照常计入。 */
function isThinShell(relSegments: string[]): boolean {
  const base = relSegments[relSegments.length - 1]
  return relSegments[relSegments.length - 2] === 'tests' && (base === 'common.rs' || base === 'batch_common.rs')
}

/** 工厂种子表清单单一来源：seed.rs 的 INSERT INTO 登记处（表名提取） */
function extractSeedTables(seedRsPath: string): string[] {
  if (!existsSync(seedRsPath)) {
    fail(`工厂种子登记处缺失：${seedRsPath}（统一测试数据库工厂，issue #751 / ADR-0084）`)
  }
  const source = readFileSync(seedRsPath, 'utf8')
  const tables = [...new Set([...source.matchAll(/INSERT\s+INTO\s+(\w+)/gi)].map((m) => m[1]))]
  if (tables.length === 0) {
    fail(`种子登记处 ${seedRsPath} 提不出任何 INSERT INTO 表名（清单漂移？）`)
  }
  return tables
}

function main(): void {
  const srcTauri = process.argv[2] ? resolve(process.argv[2]) : DEFAULT_SRC_TAURI
  const srcDir = join(srcTauri, 'src')
  const testsDir = join(srcTauri, 'tests')
  if (!existsSync(srcDir)) fail(`src-tauri 目录不存在：${srcTauri}`)

  const bannedTables = extractSeedTables(join(srcDir, 'test_support', 'seed.rs'))

  // 白名单属本仓规格（路径相对本仓 src-tauri）：仅默认扫描本仓时应用；
  // 夹具模式（位置参数）不应用——判定机制由包装测试静态导入 judge 覆盖。
  const whitelist = process.argv[2] ? [] : WHITELIST
  const files = [...walkRustFiles(srcDir), ...(existsSync(testsDir) ? walkRustFiles(testsDir) : [])]
  const { countByFile, hits } = scanFiles(files, srcTauri, bannedTables)
  const problems = judge(countByFile, hits, whitelist)
  const whitelistedTotal = whitelist.reduce((s, e) => s + (e.r1 ?? 0) + (e.r2 ?? 0) + (e.r3 ?? 0), 0)
  if (problems.length > 0) {
    console.error(
      `✗ Rust 测试守门：发现 ${problems.length} 处问题（白名单 ${whitelist.length} 组存量 ${whitelistedTotal} 处；` +
        `禁用种子表：${bannedTables.join(' ')}）\n` +
        problems.join('\n') +
        `\n建库/种子/断言唯一入口：src-tauri/src/test_support/（spec #728 / issue #751，ADR-0084）`,
    )
    process.exit(1)
  }

  console.log(
    `✅ Rust 测试守门通过（白名单 ${whitelist.length} 组存量 ${whitelistedTotal} 处待迁移；` +
      `禁用种子表 ${bannedTables.length} 张：${bannedTables.join(' ')}）`,
  )
}

/** 逐文件扫描，返回 (文件×规则) 命中数与全部命中明细（供白名单判定与输出） */
function scanFiles(
  files: string[],
  srcTauri: string,
  bannedTables: string[],
): { countByFile: Map<string, Map<1 | 2 | 3, number>>; hits: Array<{ rel: string } & Hit> } {
  const countByFile = new Map<string, Map<1 | 2 | 3, number>>()
  const hits: Array<{ rel: string } & Hit> = []
  for (const file of files.sort()) {
    const rel = relative(srcTauri, file).split('\\').join('/')
    const segments = rel.split('/')
    const underTestSupport = segments[0] === 'src' && segments[1] === 'test_support'
    const source = readFileSync(file, 'utf8')
    const masked = maskComments(source)

    // 各规则的扫描区域：测试路径整文件，产品路径仅内联 cfg(test) 块
    const regions: Region[] = isTestPath(segments)
      ? [{ start: 0, end: masked.length }]
      : extractCfgTestRegions(masked)
    if (regions.length === 0) continue

    const lineOf = (offset: number): number => {
      let line = 1
      for (let k = 0; k < offset; k++) if (masked[k] === '\n') line++
      return line
    }

    for (const region of regions) {
      const text = masked.slice(region.start, region.end)
      // 行号按原文计算：masked 与原文等长等换行，region 起点即原文偏移
      for (const hit of findHits(text, (o) => lineOf(region.start + o), bannedTables)) {
        // 规则豁免：test_support 是工厂本体，三条规则全部合法
        if (underTestSupport) continue
        if (hit.rule === 2 && isThinShell(segments)) continue // 薄皮种子合法（准入规则，ADR-0084 决策 1）
        if (!countByFile.has(rel)) countByFile.set(rel, new Map())
        const byRule = countByFile.get(rel)!
        byRule.set(hit.rule, (byRule.get(hit.rule) ?? 0) + 1)
        hits.push({ rel, ...hit })
      }
    }
  }
  return { countByFile, hits }
}

/** 白名单判定（纯函数，供包装测试静态导入复用）：返回问题清单，空 = 通过。
 *  命中与声明严格相等：超出红（违规，收敛到工厂/薄皮）；低于也红（基线过期——
 *  迁移票必须同步缩减声明数，白名单即规格）；条目文件消失或全字段为零红（漂移）。 */
export function judge(
  countByFile: Map<string, Map<1 | 2 | 3, number>>,
  hits: Array<{ rel: string } & Hit>,
  whitelist: readonly GateWhitelistEntry[] = WHITELIST,
): string[] {
  const problems: string[] = []
  const declared = new Map(whitelist.map((e) => [e.file, e]))

  // 1) 白名单条目校验：文件存在、至少一个规则计数为正
  for (const entry of whitelist) {
    const total = (entry.r1 ?? 0) + (entry.r2 ?? 0) + (entry.r3 ?? 0)
    if (total === 0) {
      problems.push(`  白名单条目 ${entry.file} 全字段为零——迁移完成后请整行移除（白名单即规格）`)
    } else if (!countByFile.has(entry.file)) {
      problems.push(`  白名单条目 ${entry.file} 已无任何命中——基线过期，请移除条目（${entry.note}）`)
    }
  }

  // 2) 逐文件严格相等判定
  const files = new Set<string>([...countByFile.keys(), ...declared.keys()])
  const RULE_NAMES = { 1: '直连建库', 2: '夹具裸SQL', 3: '默认时刻字面量' } as const
  for (const file of [...files].sort()) {
    const entry = declared.get(file)
    for (const rule of [1, 2, 3] as const) {
      const actual = countByFile.get(file)?.get(rule) ?? 0
      const expected = entry?.[`r${rule}`] ?? 0
      if (actual > expected) {
        const lines = hits.filter((h) => h.rel === file && h.rule === rule).map((h) => h.line)
        problems.push(
          `  ${file}:${lines.join(':')}\n` +
            `      规则 ${rule}（${RULE_NAMES[rule]}）命中 ${actual} 处，白名单声明 ${expected} 处` +
            (expected > 0 ? '——超出基线' : '——未入白名单') +
            `，收敛到 test_support 工厂/种子或域薄皮（ADR-0084）`,
        )
      } else if (actual < expected) {
        problems.push(
          `  ${file}  规则 ${rule}（${RULE_NAMES[rule]}）基线过期：声明 ${expected} 处、实际 ${actual} 处——` +
            `请把白名单该组缩减为 ${actual}（迁移票递减基线）`,
        )
      }
    }
  }
  return problems
}

// 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的扫描函数与白名单。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
