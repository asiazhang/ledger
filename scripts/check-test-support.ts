#!/usr/bin/env bun
// Rust 测试守门（issue #752 落地 / #758 收口 / #956 追加规则 4 / ADR-0084 决策 8），
// 四条规则：
//
// 统一测试数据库工厂（src-tauri/src/test_support/，#751 落地）是建库与种子知识的
// 唯一入口；本守门防回潮——测试代码绕开工厂直连建库、直写种子表、自抄默认时刻，
// 一律红。白名单起步（「白名单即规格」，ADR-0056 哲学）、随按域迁移票逐组清零
// （API 集成 #753、叶子域+db+sync #754、investment #755、scheduled_transactions
// #756、transaction #757、backup/内联簿记 #758），#758 收口移除白名单机制转
// 纯禁令：扫描范围内命中即红，无基线可维护。前端同构先例：
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
// 范围边界（#764 裁决）：规则 1 不辖 tests/e2e——BDD 层与工厂分层互斥
// （CONTEXT-testing「公开写入口（测试侧）」/ ADR-0086 决策 9），工厂按决策不入
// BDD 层（ADR-0084 决策 3：文件库/加密是 BDD 场景），e2e 建库走产品开库入口
// （world 持产品 DbState、boot 组文件库 init_db/open_db_in）是分层形态而非旁路，
// 命中不构成回潮信号；规则 2/3 对 tests/e2e 整目录覆盖（#764 恢复，例外见
// 下方已登记例外表）。
//
// 规则 2（禁夹具裸 SQL）：工厂种子表的 `INSERT INTO` 出现在 test_support 与
// 各域薄皮之外即红。禁用表集合单一来源 = test_support/seed.rs 的 `INSERT INTO`
// 登记处（提取其表名；工厂未来新种子自动扩展禁令，与 check-test-stubs.ts 从
// REFERENCE_DEFAULTS 提取命令清单同款，无双源漂移）；登记处提不出任何表名即红。
// 薄皮豁免按文件名形状：父目录恰为 tests 目录段且文件名为 common.rs /
// batch_common.rs——域薄皮按准入规则长期保留单域特有种子（ADR-0084 决策 1）。
// 顶屋 tests/ 下的子目录共享层（tests/api_server/common.rs 等）不豁免——文件名
// common.rs 不构成薄皮（薄皮以父目录恰为 tests 判定），命中照常计入。
// 业务表（transactions/categories/budgets 等）的 INSERT 不在本守门范围——那是
// 公开写入口纪律（测试层显式例外逐个登记）的辖域，不是建库/种子工厂的。
//
// 规则 3（禁默认时刻字面量）：`2026-01-01T00:00:00Z`（FIXED_NOW 现值）出现在
// test_support 之外的测试代码即红——夹具簿记戳由工厂种子内部发放，调用点零字面量。
// 域时刻字面量合法（时间推进是测试的行为输入，ADR-0084 决策 5），不在扫描范围；
// 文本级无法分辨意图，恰为该值的字面量一律计入（如实际作域时刻用，迁移票缩减
// 白名单时改写为其他值或引用常量）；tests/e2e 的例外见下方已登记例外表。
//
// 规则 4（禁自建通道线格式，issue #956）：测试代码不得手工构造通道段/清单
//（`SegmentEntry {` / `ChannelManifest {` 字面构造），也不得在**引用了通道面**
// 的文件里自建摘要实现（`Sha256` / `sha2::`）。通道线格式的成帧单点是
// `test_support::channel::publish_raw_segment`（内部消费产品侧 `channel::sha256_hex`
// 唯一实现）——测试侧自建会让段命名规则、清单版本与「尺寸/摘要算在封包后字节
// 上」的口径各自漂移一份（#956 成因：两处夹具都错把尺寸/摘要算在 payload 上，
// 明文模式下恰好相等而掩盖）。
// 「引用了通道面」的判定：同文件出现 `ChannelManifest` / `SegmentEntry` /
// `EnvelopeMode` / `publish_raw_segment` 任一标识。此作用域限定是为了不误伤
// 与通道无关的摘要用法（如 `src/bin/ledger-perf/tests.rs` 的生成器确定性 DB
// 摘要——它自带 sha2 但是另一件事）。
//
// tests/e2e 已登记例外（#764 裁决，登记处 = ADR-0086 修订注记）：e2e 侧无法
// 收敛的存量命中逐条登记（文件 + 规则 + 预期命中数 + 动机一句话），与
// ADR-0073 例外白名单纪律同构——「例外显式登记在案，防止被无意复制」。
// 严格相等校验：e2e 实际命中与表不全等即红——多出的命中（未登记新旁路）、
// 少掉的命中（例外已收敛，应删条目）都红，与白名单即规格（ADR-0056）同哲学。
// 注意：公开写入口纪律辖域的直置（UPDATE/DELETE/业务表 INSERT，无公开入口、
// 代码处附动机注释者）不在本守门范围，其登记处同样是 ADR-0086 修订注记。
//
// 扫描边界（文本级，注释掩码后匹配，注释里的形态不计数）：
// - 「测试代码」= ① 路径含 tests 目录段或文件名为 tests.rs 的文件（域外挂测试
//   模块/目录与顶层集成测试，check-structure.ts 同款约定）整文件；
//   ② 其余产品文件内的 `#[cfg(test)] mod <name> { … }` 内联块（括号配对，词法
//   跳过字符串与注释）。产品代码本体不扫——产品开库、产品写表、产品默认时刻
//   均合法，四条规则只辖测试代码。
// - tests/e2e/** 规则 2/3 整目录覆盖（#764 恢复，#758 时暂离）：未登记命中即红；
//   规则 1 不辖（分层形态，见规则 1 范围边界）。规则 4 对 tests/e2e 照常生效
//   （e2e 正是本规则要辖的两处消费方之一）。
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
// 纯禁令语义（#758 收口）：扫描范围内任一命中即红，逐文件逐规则计数输出；无
// 白名单、无基线——需要旁路的形态先收敛进工厂/薄皮，或按显式例外纪律登记动机
// （先例：壳层写仪式例外白名单，ADR-0073）后再恢复扫描覆盖。
//
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-test-support.ts`。
// 默认校验本仓库；测试可传位置参数指向夹具：bun scripts/check-test-support.ts [src-tauri-dir]
// workspace 成员（crates/*/src 与 crates/*/tests）同在该扫描范围内（spec #1086 /
// issue #1087）——拆 crate 后测试守门不得因目录随迁而静默漏扫。
// 挂载于 scripts/check.sh 质量门槛序列；包装测试 src/__tests__/check-test-support.test.ts。

import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { pathToFileURL } from 'node:url'

const DEFAULT_SRC_TAURI = join(fileURLToPath(import.meta.url), '..', '..', 'src-tauri')

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

// ——— 四条规则的命中形态 ———
// 规则 1：裸标识符匹配（任意限定路径、裸调用与 use 引入、测试侧平行建库入口
// 如 DbState::open_in_memory 与方法形态全部命中；\b 边界排除更长标识符的子串）。
const RULE1_IDENTIFIERS = /\b(open_in_memory|init_db)\b/g
// 规则 3：FIXED_NOW 现值（字面量形态，含于字符串内）。
const RULE3_LITERAL = '2026-01-01T00:00:00Z'
// 规则 4：手工构造通道段/清单（字面构造形态），与自建摘要实现（裸标识符）。
// 段/清单构造按 `Type {`（结构体字面量）匹配：类型名单独出现（如 `use ...SegmentEntry`）
// 不命中——仅当文件里真的在拼字面量时才红。
const RULE4_LITERAL_CONSTRUCTIONS = [/\bSegmentEntry\s*\{/g, /\bChannelManifest\s*\{/g]
const RULE4_DIGEST_IDENTIFIERS = /\b(Sha256|sha2)\b/g
// 规则 4 的作用域门：同文件出现下列任一标识才启用摘要禁令（避开与通道无关的
// 摘要用法，见文件头规则 4 说明）。
const RULE4_CHANNEL_SURFACE = [/\bChannelManifest\b/, /\bSegmentEntry\b/, /\bEnvelopeMode\b/, /\bpublish_raw_segment\b/]

// tests/e2e 已登记例外（#764 裁决；登记处 = ADR-0086 修订注记，代码处附动机
// 注释）：文件 + 规则 + 预期命中数 + 动机一句话。规则 1 无 e2e 例外（不辖，
// 见文件头规则 1 范围边界）。
interface E2eException {
  file: string
  rule: Rule
  count: number
  why: string
}
const E2E_REGISTERED_EXCEPTIONS: readonly E2eException[] = [
  {
    file: 'tests/e2e/instruments_steps.rs',
    rule: 2,
    count: 3,
    why: '存量同步行夹具：公开创建入口只产 manual 行，来源随行终身不变（ADR-0036），同步来源拒删/upsert 来源不改写的被测前提依赖直置',
  },
  {
    file: 'tests/e2e/investment_trend_steps.rs',
    rule: 2,
    count: 1,
    why: '汇率历史周采样无公开落库入口：唯一写入点 sync 持久化为 pub(super) 模块私有（采集通道需 HTTP），域层无公开入口',
  },
  {
    file: 'tests/e2e/transactions_query_steps.rs',
    rule: 3,
    count: 4,
    why: '两处直置行 SQL（同日批量导入 ×1、播种 8 类 ×1）各含 created_at/updated_at 同值字面量两次：同 created_at 平局是被测前提（确定性排序 id tiebreaker），行为层逐行发放秒级时钟、跨秒即失去平局，前提无法确定性构造',
  },
]

interface Hit { rule: Rule; line: number }

/** 四条规则的编号集（判定循环与计数表的单一来源） */
const RULES = [1, 2, 3, 4] as const
type Rule = (typeof RULES)[number]

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
  for (const re of RULE4_LITERAL_CONSTRUCTIONS) {
    re.lastIndex = 0
    let m4: RegExpExecArray | null
    while ((m4 = re.exec(masked))) hits.push({ rule: 4, line: lineOf(m4.index) })
  }
  // 摘要禁令仅在文件引用了通道面时启用（作用域限定，见文件头规则 4 说明）。
  if (RULE4_CHANNEL_SURFACE.some((re) => re.test(masked))) {
    RULE4_DIGEST_IDENTIFIERS.lastIndex = 0
    let m5: RegExpExecArray | null
    while ((m5 = RULE4_DIGEST_IDENTIFIERS.exec(masked))) hits.push({ rule: 4, line: lineOf(m5.index) })
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

/**
 * workspace 成员 crate 的 Rust 根（crates/<crate>/src 与 crates/<crate>/tests）：
 * 拆 crate 后测试守门须覆盖全 workspace（spec #1086 / issue #1087），不因目录
 * 随迁而把成员 crate 的测试代码留在扫描范围之外。
 */
function memberCrateRustRoots(srcTauri: string): string[] {
  const cratesDir = join(srcTauri, 'crates')
  if (!existsSync(cratesDir)) return []
  const roots: string[] = []
  for (const entry of readdirSync(cratesDir, { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name),
  )) {
    if (!entry.isDirectory()) continue
    for (const sub of ['src', 'tests']) {
      const dir = join(cratesDir, entry.name, sub)
      if (existsSync(dir)) roots.push(dir)
    }
  }
  return roots
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

  const files = [
    ...walkRustFiles(srcDir),
    ...(existsSync(testsDir) ? walkRustFiles(testsDir) : []),
    ...memberCrateRustRoots(srcTauri).flatMap((dir) => walkRustFiles(dir)),
  ]
  const { countByFile, hits } = scanFiles(files, srcTauri, bannedTables)
  const scanned = new Set(files.map((f) => relative(srcTauri, f).split('\\').join('/')))
  const problems = violations(countByFile, hits, scanned)
  if (problems.length > 0) {
    console.error(
      `✗ Rust 测试守门：发现 ${problems.length} 处违规（纯禁令，无白名单；` +
        `禁用种子表：${bannedTables.join(' ')}）\n` +
        problems.join('\n') +
        `\n建库/种子/断言唯一入口：src-tauri/src/test_support/（spec #728 / issue #751，ADR-0084）`,
    )
    process.exit(1)
  }

  console.log(
    `✅ Rust 测试守门通过（纯禁令：白名单机制已随 #758 收口移除；` +
      `禁用种子表 ${bannedTables.length} 张：${bannedTables.join(' ')}；` +
      `tests/e2e 已登记例外 ${E2E_REGISTERED_EXCEPTIONS.length} 条，严格相等校验）`,
  )
}

/** 逐文件扫描，返回 (文件×规则) 命中数与全部命中明细（供纯禁令判定与输出） */
function scanFiles(
  files: string[],
  srcTauri: string,
  bannedTables: string[],
): { countByFile: Map<string, Map<Rule, number>>; hits: Array<{ rel: string } & Hit> } {
  const countByFile = new Map<string, Map<Rule, number>>()
  const hits: Array<{ rel: string } & Hit> = []
  for (const file of files.sort()) {
    const rel = relative(srcTauri, file).split('\\').join('/')
    const segments = rel.split('/')
    const underTestSupport = segments[0] === 'src' && segments[1] === 'test_support'
    // tests/e2e 分层形态（#764）：规则 1 不辖（工厂不入 BDD 层，建库走产品开库
    // 入口，见文件头规则 1 范围边界）；规则 2/3 整目录覆盖，例外经严格相等校验。
    const inE2e = rel.startsWith('tests/e2e/')
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
        // 规则豁免：test_support 是工厂本体，四条规则全部合法
        if (underTestSupport) continue
        if (inE2e && hit.rule === 1) continue // 规则 1 不辖 e2e（分层形态，文件头范围边界）
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

/** 纯禁令判定（#758 收口，白名单机制已移除）：src 树任一命中即违规；tests/e2e
 *  命中对照已登记例外表严格相等校验（#764；「已收敛」零命中校验只在被扫文件
 *  实际存在时生效——例外表只辖本仓 e2e 树，不辖测试夹具），返回违规清单。 */
function violations(
  countByFile: Map<string, Map<Rule, number>>,
  hits: Array<{ rel: string } & Hit>,
  scanned: Set<string>,
): string[] {
  const problems: string[] = []
  const RULE_NAMES = { 1: '直连建库', 2: '夹具裸SQL', 3: '默认时刻字面量', 4: '自建通道线格式' } as const
  for (const file of [...countByFile.keys()].sort()) {
    for (const rule of RULES) {
      const actual = countByFile.get(file)?.get(rule) ?? 0
      if (actual === 0) continue
      const lines = hits
        .filter((h) => h.rel === file && h.rule === rule)
        .map((h) => h.line)
        .sort((a, b) => a - b)
      // tests/e2e：对照已登记例外表严格相等校验（登记处 ADR-0086 修订注记）
      if (file.startsWith('tests/e2e/')) {
        const registered = E2E_REGISTERED_EXCEPTIONS.find((e) => e.file === file && e.rule === rule)
        if (registered === undefined) {
          problems.push(
            `  ${file}:${lines.join(':')}\n` +
              `      规则 ${rule}（${RULE_NAMES[rule]}）命中 ${actual} 处——未登记例外（登记处 ADR-0086 修订注记）：` +
              `公开入口存在则收敛，不存在则代码处附动机注释并登记后恢复扫描`,
          )
        } else if (actual !== registered.count) {
          problems.push(
            `  ${file}:${lines.join(':')}\n` +
              `      规则 ${rule}（${RULE_NAMES[rule]}）实际命中 ${actual} 处 ≠ 登记的 ${registered.count} 处` +
              `（${registered.why}）——命中数漂移即例外失真，请复核后同步更新代码注释、例外表与 ADR 登记`,
          )
        }
        continue
      }
      problems.push(
        `  ${file}:${lines.join(':')}\n` +
          `      规则 ${rule}（${RULE_NAMES[rule]}）命中 ${actual} 处——纯禁令（白名单已移除），` +
          `收敛到 test_support 工厂/种子或域薄皮（ADR-0084）`,
      )
    }
  }
  // 严格相等的另一侧：例外已收敛（命中清零）的登记条目红——例外表不允许
  // 滞留已失效条目（失效登记是第二份事实）
  for (const e of E2E_REGISTERED_EXCEPTIONS) {
    if (!scanned.has(e.file)) continue // 被扫树无此文件（测试夹具）：例外表不辖
    const actual = countByFile.get(e.file)?.get(e.rule) ?? 0
    if (actual === 0) {
      problems.push(
        `  ${e.file}\n` +
          `      规则 ${e.rule}（${RULE_NAMES[e.rule]}）已登记例外 ${e.count} 处实际命中 0 处——例外已收敛，` +
          `请同步删除例外表条目、代码处注释与 ADR 登记`,
      )
    }
  }
  return problems
}

// 仅直接运行时执行 main；包装测试以子进程调用（行为等价判据：只测外部可观察结果）。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
