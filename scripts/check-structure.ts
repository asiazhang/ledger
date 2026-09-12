#!/usr/bin/env bun
// 结构守门（issue #396 / ADR-0056；模型域化禁令 issue #424 / ADR-0059 决策 6）：
// 白名单式分层依赖检查。
// 分层规则：壳 → 域 → 基础设施，域永不依赖壳。白名单 = 已归位域目录 + 全部
// 基础设施（「已验证对壳层零依赖」固化为规格）；白名单内出现对壳层
// （src-tauri/src/commands/）的模块路径依赖即红——每归位一域追加一行白名单。
// 测试豁免（ADR-0056 决策 5）：外挂测试模块/目录（tests.rs 文件、tests/ 目录）
// 不参与守门——BDD/单元 fixture 合法引用壳层入口，不制造虚假违规；
// 内联 #[cfg(test)] 模块不豁免。白名单路径缺失或条目内扫不到非测试 Rust 文件
// 即红（清单漂移 fail loud）。
// 扫描边界：文本级扫描，注释与字符串/char 字面量掩码后匹配 `commands::`
// 路径引用与 `commands as` 别名引入；经别名改名的间接引用文本不可达，
// 靠评审兜底。
// 基础设施→域扫描（ADR-0071 决策 6 / #538）：基础设施模块（#1088 起住
// `crates/infra/src`，清单见 INFRA_MODULES）内的反向依赖
// 文本级扫描，与壳层扫描同款形态（掩码后匹配、fail loud、外挂测试豁免不变）。
// 匹配限定 crate 根前缀的域模块路径（`crate::`/`tauri_app_lib::` + 域目录名，
// 再随 `::`/` as `/`;`）——不裸匹配域名单词：`sync` 等域名与 std::sync /
// tokio::sync 撞名，裸词形态误报不可用，crate 根限定即 infra→域 import 的
// 文本形状；花括号列举首段（use crate::{accounts::x, …}）同可命中。
// 认许边（INFRA_DOMAIN_ALLOWED_EDGES）：基础设施→域的既有设计意图边逐条
// 留痕于本脚本（精确到文件 + 目标域，附 ADR 指针），与白名单同属「已验证
// 事实固化为规格」；清单之外的基础设施→域引用一律红。首条 db/mod.rs→backup
// 为 ADR-0032 连接层写入口置脏单点（#246）——ADR-0071 §6「落地即全绿」原
// 前提漏数此边，勘误注记见该 ADR（#538 实施时补录）。
// 业务域→同步域严形态（ADR-0101 决策 4b / 勘误 4）：业务域引用同步域（sync_engine）
// 合法面 = 契约模块 `command` ∪ 根再导出白名单 {DomainCommand, record_local,
// device_id}（各附缘由注释，认许边同款纪律）；内部模块路径（engine::/ops::/model::
// /channel::…）一律红——「重放不产本地 op」今天靠「域不知道 ops 存在」的结构巧合
// 成立，扫描把巧合变规格。作用域限业务域目录（test_support/ 测试专用边、sync_engine
// 自身、壳层 commands/ 与 tests 不在列）；文本级扫描、别名改写不可达，靠评审兜底。
// 模型域化禁令（ADR-0059 T7 / #424 收口落地，全树扫描、同样掩码与测试豁免）：
// ① 全局模型模块路径残留禁令——`crate::models` / `tauri_app_lib::models` 即红：
//    全局模型目录已随域归位消亡，防扁平命名空间复活（crate 根裸路径 `models::x`
//    与别名改写文本不可达，靠评审兜底）；
// ② 域模型 glob 再导出禁令——`pub use …model(s)::*`（域接缝或跨域拍平）与
//    域模型文件（model.rs / models.rs）内的 `pub use …::*` 聚合即红，
//    所有权必须逐类型可见（`pub(crate) use` 受限再导出与私有 `use` glob 引入
//    不在文本可辨范围，靠评审兜底）。
// ③ 原生事务语句禁令（issue #1014 / #1003 grilling 定案 7）——产品代码手写
//    `BEGIN`/`COMMIT`/`ROLLBACK` 即红，唯一合法住址 `db/tx_scope.rs`（事务原语
//    本体）；靶形态落在字符串里，扫描保留字符串、只掩码注释；外挂测试豁免不变。
// crate 边界核对（spec #1086 / issue #1087 门禁前置）：模块路径白名单之上再加
// crate 级核对——CRATES 是 workspace 成员、分层与允许依赖方向的唯一事实源；
// 成员目录（crates/*）与 CRATES 双向全等（新 crate 未登记即红）；每个成员须写
// `[lints] workspace = true` 继承六件套门禁（漏写即红——clippy 本身不会报）；
// 依赖方向按 壳 → 域 → 基础设施 单向核对；scripts/check.sh、scripts/test.sh 与
// scripts/lint-fix.sh、CI workflow 的 cargo clippy/test/fmt 命令须显式 `--workspace`
// 或词尾 `--all`（非虚拟 workspace 下默认只作用于根包，缺范围参数会静默漏检成员；
// `--all-targets` 等 `--all*` 旗标不算范围——\b 匹配会在这里假绿，故按词尾判定）。
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-structure.ts`。
// 默认校验本仓库；测试可传位置参数指向夹具：
// bun scripts/check-structure.ts [src-dir] [src-tauri-dir]
// 挂载于 scripts/check.sh 质量门槛序列与 CI（build.yml frontend job），
// 与命令注册一致性检查并列。

import { existsSync, readdirSync, readFileSync, statSync, type Stats } from 'node:fs'
import { dirname, join } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'

/** 白名单条目（ADR-0056 决策 4） */
export interface WhitelistEntry {
  path: string
  layer: Layer
  note: string
}

/** 分层词汇（白名单 layer 字段取值）：比较侧单一来源，防字面量漂移 */
export const LAYER = {
  DOMAIN: '域目录',
  INFRA: '基础设施',
} as const

export type Layer = (typeof LAYER)[keyof typeof LAYER]

/**
 * 守门白名单（ADR-0056 决策 4）：路径相对 src-tauri/src。
 * 首批 = 已归位域目录；每迁一域在此追加一行。基础设施自 #1088 起整体住
 * `crates/infra/src`（不再有根 src 路径），改由下面的 INFRA_MODULES 清单核对。
 */
export const WHITELIST: readonly WhitelistEntry[] = [
  { path: 'transaction', layer: '域目录', note: '核心交易域' },
  { path: 'scheduled_transactions', layer: '域目录', note: '定时计划域' },
  { path: 'item', layer: '域目录', note: '物品域（#397 阶段 1 归位，主体自 commands/item 随迁）' },
  { path: 'policy', layer: '域目录', note: '保单域（#398 阶段 2 归位）' },
  { path: 'budget', layer: '域目录', note: '预算域（#399 阶段 3 归位）' },
  { path: 'merchants', layer: '域目录', note: '商户域（#400 阶段 4 归位）' },
  { path: 'physical_asset', layer: '域目录', note: '实物资产域（issue #466 新建即归位，ADR-0064）' },
  { path: 'investment', layer: '域目录', note: '投资域（#401 阶段 5 归位，主体自 commands/investment 随迁；价格写入单点自 sync/persist 迁入）' },
  { path: 'accounts', layer: '域目录', note: '账户域（#404 参考数据域归位，主体自 commands/accounts 随迁）' },
  { path: 'categories', layer: '域目录', note: '分类域（#404 参考数据域归位，主体自 commands/categories 随迁）' },
  { path: 'currencies', layer: '域目录', note: '币种域（#404 参考数据域归位，清单查询自 commands/currencies 迁入）' },
  { path: 'reports', layer: '域目录', note: '报表域（#405 归位，月度汇总/分类/商户/日期极值聚合读模型，消费 transaction::amount 矩阵）' },
  { path: 'dashboard', layer: '域目录', note: '仪表盘域（#405 归位，全仓净资产跨币种折算聚合）' },
  { path: 'backup', layer: '域目录', note: '备份域（#406 归位，备份引擎自 commands/backup/core、自动备份调度自顶层 auto_backup.rs 整合随迁）' },
  { path: 'sync', layer: '域目录', note: '行情同步域（#407 归位，HTTP 爬取/东财基金净值/同步编排自 commands/sync 随迁；全量修字典翼已退役，issue #698）' },
  { path: 'sync_engine', layer: '域目录', note: '多端同步域（issue #855 新建即归位，ADR-0091 OpLog 基座；与行情同步域 sync 相邻不同域）' },
  { path: 'test_support', layer: '域目录', note: '测试支持域（统一测试数据库工厂与共享断言库，ADR-0084 / #751；依赖域与基础设施合法，对壳层零依赖）' },
]

/**
 * 基础设施 crate 的模块清单（spec #1086 / issue #1088）：路径相对
 * `src-tauri/crates/infra/src`。数据库、错误、设置、文件工具、日志、事件、
 * 信号、闭集与壳层统一读写入口全量归位于此——它们对根包只以再导出面存在，
 * 故模块级守门（对壳层零依赖、基础设施→域认许边）随之落到 crate 根下扫描。
 */
export const INFRA_MODULES: readonly WhitelistEntry[] = [
  { path: 'db', layer: '基础设施', note: '数据库连接与 schema 守卫（#1127 起 mod.rs 只留声明与再导出，按职责分 migrate / connection / runtime 三文件；时间与身份工厂暂住 mod.rs，#1128 升顶层 ids）' },
  { path: 'signals.rs', layer: '基础设施', note: '信号映射（ADR-0044）' },
  { path: 'error.rs', layer: '基础设施', note: '错误' },
  { path: 'settings.rs', layer: '基础设施', note: '设置' },
  { path: 'fs_util.rs', layer: '基础设施', note: '文件级原子操作工具（备份与 DataLocation 搬迁共用，#408 纳入守门）' },
  { path: 'logger.rs', layer: '基础设施', note: '日志初始化与滚动清理（#408 纳入守门）' },
  { path: 'events.rs', layer: '基础设施', note: '事件发射机制（ADR-0054，#408 纳入守门）' },
  { path: 'closed_set.rs', layer: '基础设施', note: '闭集字符串枚举宏（ADR-0108；模式先例 signals.rs write_op_set!，ADR-0102）' },
  { path: 'write_entry.rs', layer: '基础设施', note: '壳层统一写入口（ADR-0073，spec #523）' },
  { path: 'read_entry.rs', layer: '基础设施', note: '壳层统一读入口（ADR-0104，spec #1009）' },
  { path: 'redact.rs', layer: '基础设施', note: 'IPC 载荷脱敏（issue #1087 首位成员）' },
  { path: 'test_utils.rs', layer: '基础设施', note: '测试器具（捕获 tracing 事件的 Layer / 闸门式假发射器，#1088 随类型身份约束归位；`#[doc(hidden)]`，生产路径不得消费）' },
]

/** 基础设施 crate 的模块根（相对 src-tauri），与 CRATES 的 ledger-infra.dir 同源。 */
export const INFRA_SRC_REL = 'crates/infra/src'

/**
 * crate 分层词汇（crate 边界核对用）：壳 → 域 → 基础设施单向。
 * 与上面的 `LAYER`（单 crate 内的**模块路径**分层：域目录 / 基础设施）刻意分开——
 * 两者是不同粒度的事实源，同名值不合并（合并只会让任一侧语义被动漂移）。
 */
export const CRATE_LAYER = {
  SHELL: '壳',
  DOMAIN: '域',
  INFRA: '基础设施',
} as const

export type CrateLayer = (typeof CRATE_LAYER)[keyof typeof CRATE_LAYER]

/** crate 依赖方向优先级（数值大者可依赖数值小者）：壳 → 域 → 基础设施。 */
const CRATE_LAYER_RANK: Record<CrateLayer, number> = {
  [CRATE_LAYER.SHELL]: 2,
  [CRATE_LAYER.DOMAIN]: 1,
  [CRATE_LAYER.INFRA]: 0,
}

/** crate 边界条目（单一事实源，spec #1086 / issue #1087）：dir 相对 src-tauri。 */
export interface CrateEntry {
  name: string
  dir: string
  layer: CrateLayer
  note: string
}

/**
 * crate 边界清单：workspace 成员、分层与允许的依赖方向（壳 → 域 → 基础设施）
 * 的唯一事实源——结构守门据此核对成员登记、门禁继承与依赖方向；每拆一个域
 * crate 在此追加一行（与 WHITELIST 同为「已验证事实固化为规格」）。
 */
export const CRATES: readonly CrateEntry[] = [
  {
    name: 'tauri-app',
    dir: '.',
    layer: CRATE_LAYER.SHELL,
    note: 'tauri 应用包：命令注册扫描、IPC/HTTP 壳与集成测试入口；过渡期仍承载尚未迁出的域，随 P1–P5 逐域迁出',
  },
  {
    name: 'ledger-infra',
    dir: 'crates/infra',
    layer: CRATE_LAYER.INFRA,
    note: '基础设施 crate（#1088 全量归位：数据库/错误/设置/文件工具/日志/事件/信号/闭集/壳层统一读写入口 + 载荷脱敏；对根包只以再导出面存在，基础设施→域生产边为 0——提交点后置动作经注册点反转，接线在壳层启动）',
  },
]

/** 成员目录约定（workspace glob）：新增 crate 只需落在此目录下即自动入 workspace。 */
const MEMBER_DIR_GLOB = 'crates/*'

/** 易 panic 构造六件套（ADR-0060）：workspace 级声明的键集（单一来源）。 */
const PANIC_LINT_KEYS = [
  'unwrap_used',
  'expect_used',
  'panic',
  'todo',
  'unimplemented',
  'unreachable',
] as const

/** 显式声明 workspace 范围的 cargo 命令宿主（静态检查与测试命令覆盖全成员）。 */
const WORKSPACE_COMMAND_FILES = [
  'scripts/check.sh',
  'scripts/test.sh',
  'scripts/lint-fix.sh',
  '.github/workflows/build.yml',
] as const

/** workspace 范围参数：`--workspace` 或 `--all`（后者仅在词尾，防止 `--all-targets` 假绿）。 */
const WORKSPACE_SCOPE_PATTERN = /(?:^|\s)--workspace(?:\s|$)/
const ALL_SCOPE_PATTERN = /(?:^|\s)--all(?:\s|$)/

/** 壳层依赖形态：模块路径引用（crate::commands::x / commands::x）与别名引入 */
const SHELL_DEP_PATTERN = /\bcommands\s*::|\bcommands\s+as\b/

/** 已归位域目录名（自白名单派生，单一事实源）；按长度降序防前缀吞匹配 */
const DOMAIN_NAMES: string[] = WHITELIST.filter((w) => w.layer === LAYER.DOMAIN)
  .map((w) => w.path)
  .sort((a, b) => b.length - a.length)

/**
 * 基础设施→域依赖形态（ADR-0071 决策 6 / #538）：crate 根前缀 + 域目录名，
 * 再随 `::`（路径引用）、` as `（别名引入）或 `;`（模块自身导入）；
 * 捕获组 1 = 目标域名（认许边匹配用）。`\{?\s*` 容纳花括号列举首段
 * （use crate::{accounts::x, …}）。
 */
const INFRA_DOMAIN_DEP_PATTERN = new RegExp(
  `\\b(?:crate|tauri_app_lib)\\s*::\\s*\\{?\\s*(${DOMAIN_NAMES.join('|')})\\b(?:\\s*::|\\s+as\\b|\\s*;)`,
)

/** 认许边条目：基础设施文件相对路径 + 目标域目录名 + 成因留痕 */
interface InfraDomainEdge {
  file: string
  domain: string
  reason: string
}

/**
 * 认许边（ADR-0071 决策 6 + §6 勘误注记 / #538）：基础设施→域的既有设计
 * 意图边，与白名单同属「已验证事实固化为规格」——逐条精确到白名单条目内
 * 文件相对路径（相对 `crates/infra/src`，自 #1088 归位起） + 目标域目录名，
 * 新增条目须附 ADR 指针与成因；清单之外的引用一律红。
 *
 * #1088 挂载点清点（「数量有记录、不新增」）：原首条 `db/mod.rs→backup`
 * （ADR-0032 连接层提交点置脏单点，#246）在生产代码里被注册点反转消除——
 * 基础设施只留调用时机、备份域提供实现、壳层启动接线，crate 依赖图不再有
 * 基础设施→业务域边；清单因此由 5 条降为 4 条，且余下 4 条全是内联 cfg(test)
 * 经测试工厂建库的测试专用边（生产挂载点 0 条）。
 */
const INFRA_DOMAIN_ALLOWED_EDGES: readonly InfraDomainEdge[] = [
  {
    file: 'settings.rs',
    domain: 'test_support',
    reason: 'ADR-0084 迁移状态段 + ADR-0071 决策 6：内联 cfg(test) 测试经测试工厂建库/取常量（#758 收口），测试专用边、非产品依赖',
  },
  {
    file: 'logger.rs',
    domain: 'test_support',
    reason: 'ADR-0084 迁移状态段 + ADR-0071 决策 6：内联 cfg(test) 测试经测试工厂建库（#758 收口），测试专用边、非产品依赖',
  },
  {
    file: 'write_entry.rs',
    domain: 'test_support',
    reason: 'ADR-0084 迁移状态段 + ADR-0071 决策 6：内联 cfg(test) 测试经测试工厂建库/簿记戳引用 FIXED_NOW（#758 收口），测试专用边、非产品依赖',
  },
  {
    file: 'read_entry.rs',
    domain: 'test_support',
    reason: 'ADR-0084 迁移状态段 + ADR-0071 决策 6：内联 cfg(test) 测试经测试工厂建库/种子（#758 收口），测试专用边、非产品依赖',
  },
]

/** 规则①形态：全局模型模块路径（全局目录已消亡，任何引用即残留） */
const GLOBAL_MODEL_PATH_PATTERN = /\b(?:crate|tauri_app_lib)\s*::\s*models\b/

/** 规则②形态：模型模块的 glob 再导出——域接缝 `pub use model::*` 与
 *  跨域/旧目录同名拍平 `pub use …::models::*`；逐类型花括号列举不命中 */
const MODEL_GLOB_REEXPORT_PATTERN = /\bpub\s+use\s+[\w:]*\bmodels?\b\s*::\s*\*/

/** 规则②形态：任意 glob 再导出（仅用于域模型文件内的聚合扫描） */
const MODEL_FILE_GLOB_PATTERN = /\bpub\s+use\s+[\w:]*\*/

/** 规则③形态：产品代码原生事务语句（issue #1014 / #1003 grilling 定案 7）——
 *  事务壳（无条件自持 `hold_transaction` / 嵌套感知 `ensure_transaction`）归
 *  基础设施 `db::tx_scope`，其余位置手写 `BEGIN` / `COMMIT` / `ROLLBACK` 即红。
 *  靶形态落在字符串字面量里，扫描须 `keepLiterals=true`（只掩码注释）。 */
const NATIVE_TX_STMT_PATTERN = /\bexecute\s*\(\s*"(?:BEGIN|COMMIT|ROLLBACK)\b/

/** 原生事务语句唯一合法住址（事务原语本体，issue #1014；#1088 起住基础设施 crate） */
const NATIVE_TX_STMT_ALLOWED = `${INFRA_SRC_REL}/db/tx_scope.rs`

/** 业务域→同步域引用锚点（crate 根前缀限定；掩码后匹配） */
const SYNC_ENGINE_REF_PATTERN = /\b(?:crate|tauri_app_lib)\s*::\s*sync_engine\b/

/** 同步域契约模块名（业务域唯一可引的内部模块，ADR-0101 决策 4b） */
const SYNC_CONTRACT_MODULE = 'command'

/**
 * 同步域根再导出白名单（业务域引用同步域根符号的合法面，ADR-0101 勘误 4）：
 * 每条附缘由——认许边同款纪律，新增合法符号须在此留痕。
 */
const SYNC_ROOT_ALLOWED_SYMBOLS: ReadonlyMap<string, string> = new Map([
  ['DomainCommand', '同步重放契约：op 载荷信封（域命令类型经此进 op）'],
  ['record_local', '本机 op 产出单点（行为编排入口随写事务追加 op）'],
  ['device_id', '本机 DeviceId 读取（域侧簿记戳/版本列共用接缝）'],
])

/** 花括号列举内不合法的条目头（深度 0 逐条切分后取首个标识符；`self`/契约模块/
 *  白名单符号合法）。返回违规条目头，供调用方构造命中。 */
function disallowedBraceEntries(body: string): string[] {
  const out: string[] = []
  let depth = 0
  let current = ''
  const flush = (): void => {
    const head = current.trim().split(/[\s:{]/)[0] ?? ''
    if (
      head !== '' &&
      head !== 'self' &&
      head !== SYNC_CONTRACT_MODULE &&
      !SYNC_ROOT_ALLOWED_SYMBOLS.has(head)
    ) {
      out.push(head)
    }
    current = ''
  }
  for (const ch of body) {
    if (ch === '{' || ch === '(' || ch === '[') depth++
    else if (ch === '}' || ch === ')' || ch === ']') depth--
    if (ch === ',' && depth === 0) flush()
    else current += ch
  }
  flush()
  return out
}

/**
 * 业务域→同步域严形态扫描（ADR-0101 决策 4b / 勘误 4）：返回引用同步域内部件
 * （非契约模块、非白名单根符号）的命中。文本级扫描（掩码注释与字面量）。
 */
export function scanSyncEngineRefs(text: string): ScanHit[] {
  const masked = maskNonCode(text)
  const rawLines = text.split('\n')
  const hits: ScanHit[] = []
  const lineOf = (index: number): number => (masked.slice(0, index).match(/\n/g)?.length ?? 0) + 1
  const push = (index: number, match: string): void => {
    const line = lineOf(index)
    hits.push({ line, text: (rawLines[line - 1] ?? '').trim(), match, captured: undefined })
  }
  for (const m of masked.matchAll(new RegExp(SYNC_ENGINE_REF_PATTERN, 'g'))) {
    const start = m.index ?? 0
    const tail = masked.slice(start + m[0].length)
    const afterModule = /^\s*::\s*/.exec(tail)
    if (!afterModule) {
      // 无 `::` 子路径：根模块自身导入（`use crate::sync_engine;`）合法；
      // `use crate::sync_engine as se;` 的别名引入一律红——别名改写会让后续
      // 引用文本不可达（与既有壳层扫描的 `commands as` 同款堵漏）。
      if (/^\s+as\b/.test(tail)) push(start, `${m[0]} as …`)
      continue
    }
    const cursor = start + m[0].length + afterModule[0].length
    if (masked[cursor] === '{') {
      // 根花括号列举：跨行取匹配闭括号后逐条判合法。
      let depth = 0
      let close = -1
      for (let i = cursor; i < masked.length; i++) {
        if (masked[i] === '{') depth++
        else if (masked[i] === '}') {
          depth--
          if (depth === 0) {
            close = i
            break
          }
        }
      }
      const body = masked.slice(cursor + 1, close === -1 ? masked.length : close)
      for (const entry of disallowedBraceEntries(body)) {
        push(start, `sync_engine::{…${entry}…}`)
      }
      continue
    }
    const segment = /^([A-Za-z_][A-Za-z0-9_]*)/.exec(masked.slice(cursor))
    if (!segment) {
      // `sync_engine::*` 等非具名形态：不在合法面（白名单逐符号），一律红。
      push(start, masked.slice(start, cursor + 1))
      continue
    }
    const name = segment[1]
    if (name === SYNC_CONTRACT_MODULE || SYNC_ROOT_ALLOWED_SYMBOLS.has(name)) continue
    push(start, `sync_engine::${name}`)
  }
  return hits
}

/** 域模型文件（ADR-0059 目标形状：每域一个 model.rs；定时计划域为先例名 models.rs） */
function isModelFile(relPath: string): boolean {
  const segments = relPath.split('/')
  const file = segments[segments.length - 1]
  return file === 'model.rs' || file === 'models.rs'
}

/** 测试豁免形态（ADR-0056 决策 5）：tests.rs 文件与 tests/ 目录 */
function isTestFile(relPath: string): boolean {
  const segments = relPath.split('/')
  const file = segments[segments.length - 1]
  return file === 'tests.rs' || segments.slice(0, -1).includes('tests')
}

/**
 * 掩码 Rust 源文本中的注释与字符串/char 字面量：内容替换为等长空白
 * （保留换行与列位，行号不变），使依赖扫描只落在真实代码上。
 * 处理形态：行注释（//、///、//!）、块注释（/* .. *&#47;，可嵌套）、
 * 普通字符串（含转义）、原始字符串 r"…" / r#"…"#（多级 #）、
 * char 字面量（'a'、'\n'、'\u{…}'）；生命周期标注（'a）按非字面量处理。
 * `keepLiterals=true` 时保留字符串/char 字面量内容、只掩码注释——用于靶形态
 * 落在字符串里的扫描（原生事务语句 `execute("BEGIN")`，issue #1014）。
 */
export function maskNonCode(text: string, keepLiterals = false): string {
  const out = text.split('')
  const n = text.length
  const blank = (from: number, to: number): void => {
    for (let k = from; k < to && k < n; k++) if (out[k] !== '\n') out[k] = ' '
  }
  let i = 0
  while (i < n) {
    const c = text[i]
    if (c === '/' && text[i + 1] === '/') {
      // 行注释（含 /// 与 //!）到行尾
      const end = text.indexOf('\n', i)
      const stop = end === -1 ? n : end
      blank(i, stop)
      i = stop
    } else if (c === '/' && text[i + 1] === '*') {
      // 块注释，Rust 可嵌套
      let depth = 1
      let j = i + 2
      while (j < n && depth > 0) {
        if (text[j] === '/' && text[j + 1] === '*') {
          depth++
          j += 2
        } else if (text[j] === '*' && text[j + 1] === '/') {
          depth--
          j += 2
        } else {
          j++
        }
      }
      blank(i, j)
      i = j
    } else if (c === '"') {
      // 普通字符串：跳过转义对
      let j = i + 1
      while (j < n) {
        if (text[j] === '\\') j += 2
        else if (text[j] === '"') {
          j++
          break
        } else j++
      }
      if (!keepLiterals) blank(i, j)
      i = j
    } else if (c === 'r' && (text[i + 1] === '"' || (text[i + 1] === '#' && text[i + 2] === '"'))) {
      // 原始字符串 r"…" / r#"…"# / r##"…"##；前一字 符为标识符成分时是普通名字（如 for），不误伤
      const prev = i > 0 ? text[i - 1] : ''
      if (/[A-Za-z0-9_]/.test(prev)) {
        i++
        continue
      }
      let hashes = 0
      let j = i + 1
      while (text[j] === '#') {
        hashes++
        j++
      }
      const close = '"' + '#'.repeat(hashes)
      const end = text.indexOf(close, j + 1)
      const stop = end === -1 ? n : end + close.length
      if (!keepLiterals) blank(i, stop)
      i = stop
    } else if (c === "'") {
      // char 字面量 vs 生命周期：有闭引号为字面量，否则是生命周期标注（'a）
      let j = i + 1
      if (text[j] === '\\') {
        j++
        if (text[j] === '{') {
          const e = text.indexOf('}', j)
          j = e === -1 ? n : e + 1
        } else {
          j++
        }
      } else {
        j++
      }
      if (text[j] === "'") {
        const stop = j + 1
        if (!keepLiterals) blank(i, stop)
        i = stop
      } else {
        i++
      }
    } else {
      i++
    }
  }
  return out.join('')
}

/** 单条扫描命中：行号（1 起算）、原文行、匹配文本、捕获组 1
 *  （无捕获组时 undefined，基础设施→域形态为目标域名） */
export interface ScanHit {
  line: number
  text: string
  match: string
  captured: string | undefined
}

/** 扫描单个 Rust 文本（掩码注释与字符串/char 字面量）：返回命中指定形态的
 *  行号（1 起算）与原文；形态缺省为壳层依赖（白名单分层检查的既有行为）。
 *  `keepLiterals=true` 保留字符串/char 字面量内容、只掩码注释——靶形态落在
 *  字符串里的扫描（原生事务语句，issue #1014） */
export function scanRustSource(
  text: string,
  pattern: RegExp = SHELL_DEP_PATTERN,
  keepLiterals = false,
): ScanHit[] {
  const hits: ScanHit[] = []
  const masked = maskNonCode(text, keepLiterals)
  const maskedLines = masked.split('\n')
  const rawLines = text.split('\n')
  for (let i = 0; i < maskedLines.length; i++) {
    const m = maskedLines[i].match(pattern)
    if (m) hits.push({ line: i + 1, text: rawLines[i].trim(), match: m[0], captured: m[1] })
  }
  return hits
}

/** 收集到的 Rust 文件引用：绝对路径 + 相对路径（输出与报文用） */
interface RustFileRef {
  abs: string
  rel: string
}

/** 递归收集目录下全部 .rs 文件（跳过测试豁免形态），相对路径排序保证输出确定 */
function collectRustFiles(dir: string, relBase: string): RustFileRef[] {
  const out: RustFileRef[] = []
  for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name),
  )) {
    const abs = join(dir, entry.name)
    const rel = relBase ? `${relBase}/${entry.name}` : entry.name
    if (isTestFile(rel)) continue
    if (entry.isDirectory()) out.push(...collectRustFiles(abs, rel))
    else if (entry.name.endsWith('.rs')) out.push({ abs, rel })
  }
  return out
}

/** 取 TOML 段内容（段头到下一个段头之间，不含段头行）；段不存在返回 null。 */
function manifestSection(text: string, section: string): string | null {
  const lines = text.split('\n')
  const header = `[${section}]`
  let start = -1
  for (let i = 0; i < lines.length; i++) {
    const trimmed = lines[i].trim()
    if (start === -1) {
      if (trimmed === header) start = i + 1
    } else if (trimmed.startsWith('[')) {
      return lines.slice(start, i).join('\n')
    }
  }
  return start === -1 ? null : lines.slice(start).join('\n')
}

/** 清单 [package] name（缺失返回 null）。 */
function manifestPackageName(manifest: string): string | null {
  const section = manifestSection(manifest, 'package')
  const m = section?.match(/(?:^|\n)\s*name\s*=\s*"([^"]+)"/)
  return m ? m[1] : null
}

/**
 * 清单声明的**生产**依赖 crate 名（含 target 变体；`[dependencies.x]` 子表形态）。
 * 只取依赖表本身，段落其余内容不参与——供 crate 依赖方向核对使用。
 *
 * 刻意排除 `[dev-dependencies]`：spec #1086 明文裁决「数据库 ↔ 核心交易域的双向
 * 引用是测试专用边，用 dev-dependency 环解决（Cargo 允许）」，测试工厂与器具以
 * dev-dependency 形态供各域/基础设施复用（#1088 实测：环成立，测试目标与生产依赖
 * 图分离）；生产依赖方向仍按「壳 → 域 → 基础设施」单向核对，`crates/infra` 的
 * `[dev-dependencies] tauri-app` 是本票测试专用边的落点。
 */
function declaredProductionDependencyNames(manifest: string): string[] {
  const names = new Set<string>()
  const tableRe = /^(?:target\..+\.)?dependencies(?:\.([A-Za-z0-9_-]+))?$/
  let current = ''
  for (const raw of manifest.split('\n')) {
    const header = raw.trim().match(/^\[([^\]]+)\]$/)
    if (header) {
      current = header[1]
      const sub = tableRe.exec(current)
      if (sub?.[1]) names.add(sub[1])
      continue
    }
    const sub = tableRe.exec(current)
    if (!sub || sub[1]) continue
    const key = raw.trim().match(/^([A-Za-z0-9_-]+)\s*=/)
    if (key) names.add(key[1])
  }
  return [...names]
}

/** [lints] 段声明 workspace 继承（`workspace = true`）——六件套门禁的继承接线。 */
function inheritsWorkspaceLints(manifest: string): boolean {
  const section = manifestSection(manifest, 'lints')
  return section !== null && /(?:^|\n)\s*workspace\s*=\s*true\b/.test(section)
}

/** `crates/` 下含 Cargo.toml 的成员 crate 目录（相对 src-tauri，排序保证输出确定）。 */
function memberCrateDirs(srcTauriDir: string): string[] {
  const cratesDir = join(srcTauriDir, 'crates')
  if (!existsSync(cratesDir)) return []
  return readdirSync(cratesDir, { withFileTypes: true })
    .filter((e) => e.isDirectory() && existsSync(join(cratesDir, e.name, 'Cargo.toml')))
    .map((e) => `crates/${e.name}`)
    .sort()
}

/**
 * crate 边界核对（spec #1086 / issue #1087 门禁前置）：workspace 成员登记、
 * 六件套 deny 门禁继承、依赖方向（壳 → 域 → 基础设施）与静态检查/测试命令
 * 的 workspace 覆盖，全部 fail loud、删除即变红：
 * - 成员漏写 `[lints] workspace = true` → 门禁静默消失，clippy 仍绿，本核对红；
 * - `crates/` 下新增 crate 未登记 CRATES → 边界知识分裂，本核对红；
 * - cargo 命令缺 `--workspace` → 默认只作用于根包，本核对红。
 */
function checkCrateBoundaries(srcTauriDir: string): string[] {
  const problems: string[] = []
  const repoRoot = dirname(srcTauriDir)
  const rootManifestPath = join(srcTauriDir, 'Cargo.toml')
  if (!existsSync(rootManifestPath)) {
    problems.push(`✗ crate 边界：workspace 根清单不存在：${rootManifestPath}`)
    return problems
  }
  const rootManifest = readFileSync(rootManifestPath, 'utf8')

  // ① workspace 骨架 + 成员目录 glob（新增 crate 自动成为 workspace 成员）
  const workspaceSection = manifestSection(rootManifest, 'workspace')
  if (workspaceSection === null) {
    problems.push(
      '✗ crate 边界：src-tauri/Cargo.toml 缺 [workspace] 段——Rust 根须为 workspace 根（spec #1086）',
    )
  } else if (!workspaceSection.includes(`"${MEMBER_DIR_GLOB}"`)) {
    problems.push(
      `✗ crate 边界：[workspace] members 未包含 "${MEMBER_DIR_GLOB}"——成员目录须用 glob 纳入，` +
        '新增 crate 自动入 workspace，漏项即静默漏检',
    )
  }

  // ② 六件套门禁的唯一声明处（workspace 级）
  const clippyLints = manifestSection(rootManifest, 'workspace.lints.clippy')
  if (clippyLints === null) {
    problems.push(
      '✗ 门禁继承：[workspace.lints.clippy] 缺失——六件套 deny 门禁的唯一声明处（ADR-0060 / spec #1086）',
    )
  } else {
    for (const key of PANIC_LINT_KEYS) {
      if (!new RegExp(`(?:^|\\n)\\s*${key}\\s*=\\s*"deny"`).test(clippyLints)) {
        problems.push(`✗ 门禁继承：[workspace.lints.clippy] 缺 ${key} = "deny"（ADR-0060 六件套）`)
      }
    }
  }

  // ③ 根包同为 workspace 成员，须继承门禁
  if (!inheritsWorkspaceLints(rootManifest)) {
    problems.push(
      '✗ 门禁继承：workspace 根包缺 [lints] workspace = true——根包同受六件套约束（ADR-0060）',
    )
  }

  // ④ 成员登记：磁盘成员目录 ↔ CRATES 双向核对（新 crate 未登记即红）
  const onDisk = memberCrateDirs(srcTauriDir)
  const registeredMemberDirs = CRATES.filter((c) => c.dir.startsWith('crates/'))
    .map((c) => c.dir)
    .sort()
  for (const dir of onDisk) {
    if (!CRATES.some((c) => c.dir === dir)) {
      problems.push(
        `✗ crate 边界：成员 crate 未登记 CRATES：${dir}\n` +
          '    新增 crate 后须在 scripts/check-structure.ts 的 CRATES 追加一行（分层 + 注释），' +
          '否则边界知识分裂成两份、依赖方向失守',
      )
    }
  }
  for (const dir of registeredMemberDirs) {
    if (!onDisk.includes(dir)) {
      problems.push(`✗ crate 边界：CRATES 登记的成员目录不存在：${dir}（清单漂移 fail loud）`)
    }
  }

  // ⑤ 每个 crate：清单存在、包名一致、门禁继承、依赖方向单向
  for (const crate of CRATES) {
    const isRoot = crate.dir === '.'
    const manifestPath = isRoot ? rootManifestPath : join(srcTauriDir, crate.dir, 'Cargo.toml')
    if (!existsSync(manifestPath)) {
      problems.push(`✗ crate 边界：crate 清单不存在：${crate.name}（${crate.dir}）`)
      continue
    }
    const manifest = isRoot ? rootManifest : readFileSync(manifestPath, 'utf8')
    const name = manifestPackageName(manifest)
    if (name !== crate.name) {
      problems.push(
        `✗ crate 边界：CRATES 登记名 ${crate.name} 与清单包名 ${name ?? '（缺 name）'} 不一致（${crate.dir}）`,
      )
    }
    if (!isRoot && !inheritsWorkspaceLints(manifest)) {
      problems.push(
        `✗ 门禁继承：成员 crate ${crate.name} 缺 [lints] workspace = true（${crate.dir}/Cargo.toml）\n` +
          '    缺失即六件套 deny 门禁静默消失而 clippy 依然全绿——删除继承行即变红（ADR-0060 / spec #1086）',
      )
    }
    // 依赖方向只看生产依赖（dev-dependency 环是 spec #1086 明文裁决的测试专用边，
    // 见 declaredProductionDependencyNames 注释）。
    for (const dep of declaredProductionDependencyNames(manifest)) {
      const target = CRATES.find((c) => c.name === dep)
      if (target && CRATE_LAYER_RANK[target.layer] > CRATE_LAYER_RANK[crate.layer]) {
        problems.push(
          `✗ crate 依赖方向：${crate.name}（${crate.layer}）依赖 ${target.name}（${target.layer}）\n` +
            '    分层规则：壳 → 域 → 基础设施单向；被依赖逻辑应下沉到更低层（spec #1086）',
        )
      }
    }
  }

  // ⑥ 静态检查与测试命令覆盖全成员（缺 --workspace 即静默漏检成员）
  for (const rel of WORKSPACE_COMMAND_FILES) {
    const abs = join(repoRoot, rel)
    if (!existsSync(abs)) {
      problems.push(`✗ workspace 命令覆盖：宿主文件不存在：${rel}`)
      continue
    }
    const lines = readFileSync(abs, 'utf8').split('\n')
    lines.forEach((line, i) => {
      if (line.trim().startsWith('#')) return // 注释行（含 workflow 说明）不算命令
      // 逐条命令核对（一行可有 `cargo fmt … && cargo clippy …` 多条：只看首个
      // 匹配会把未覆盖的 clippy 放过去）；命令段截到下一个 shell 控制符为止。
      const re = /\bcargo\s+(clippy|test|fmt)\b/g
      let m: RegExpExecArray | null
      while ((m = re.exec(line))) {
        const rest = line.slice(m.index)
        const end = rest.search(/&&|;|\|/)
        const segment = end === -1 ? rest : rest.slice(0, end)
        // `--all` 仅在词尾才算 workspace 别名：`--all-targets` / `--all-features`
        // 的 `\b` 落在 `-` 前，用 \b 会假绿（本核对要拦的正是这一形态）。
        if (WORKSPACE_SCOPE_PATTERN.test(segment) || ALL_SCOPE_PATTERN.test(segment)) continue
        problems.push(
          `✗ workspace 命令覆盖：${rel}:${i + 1} cargo ${m[1]} 缺 --workspace` +
            '（非虚拟 workspace 下默认只作用于根包，会静默漏检成员 crate）\n' +
            `    ${line.trim()}`,
        )
      }
    })
  }

  return problems
}

/**
 * 模块清单核对（ADR-0056 决策 4）：清单条目必须存在且扫得到非测试 Rust 文件
 * （清单漂移 fail loud），条目内对壳层零依赖；基础设施条目另核
 * 基础设施→域认许边（ADR-0071 决策 6 / #538），业务域条目另核业务域→同步域
 * 严形态（ADR-0101 决策 4b）。返回扫到的非测试文件数；返回 0 由调用方统一拒绝
 * （拒绝以空集假绿通过）。清单与路径基准分离，使域目录（根 src）与基础设施
 * crate（`crates/infra/src`）共用同一份核对逻辑与同一份认许边清单。
 */
function scanModuleEntries(
  entries: readonly WhitelistEntry[],
  baseDir: string,
  problems: string[],
): number {
  let scanned = 0
  for (const w of entries) {
    const abs = join(baseDir, w.path)
    let stat: Stats | undefined
    try {
      stat = statSync(abs)
    } catch {
      stat = undefined
    }
    if (!stat) {
      problems.push(`✗ 白名单路径不存在：${w.path}（${w.layer}：${w.note}）——目录改名/迁移后未同步守门清单`)
      continue
    }
    const files: RustFileRef[] = stat.isDirectory()
      ? collectRustFiles(abs, w.path)
      : [{ abs, rel: w.path }]
    if (files.length === 0) {
      problems.push(
        `✗ 白名单条目扫不到非测试 Rust 文件：${w.path}（${w.layer}：${w.note}）——` +
          `全部是测试豁免形态或已空，清单与目录形状漂移`,
      )
      continue
    }
    scanned += files.length
    const isInfra = w.layer === LAYER.INFRA
    // 业务域→同步域严形态作用域：域目录，除同步域自身与测试支持域
    // （test_support→sync_engine 为测试专用边，登记处 ADR-0084 迁移状态段）。
    const isBusinessDomain =
      w.layer === LAYER.DOMAIN && w.path !== 'sync_engine' && w.path !== 'test_support'
    for (const f of files) {
      const source = readFileSync(f.abs, 'utf8')
      for (const hit of scanRustSource(source)) {
        problems.push(
          `✗ 反向依赖：${w.path} 层（${w.note}）引用壳层 → ${f.rel}:${hit.line}（${hit.match}）\n` +
            `    ${hit.text}\n` +
            `    分层规则：壳 → 域 → 基础设施，域永不依赖壳（ADR-0056）；` +
            `被依赖逻辑应下沉到域目录或基础设施，或本次迁移应把该文件一并归位`,
        )
      }
      if (isInfra) {
        for (const hit of scanRustSource(source, INFRA_DOMAIN_DEP_PATTERN)) {
          const allowed = INFRA_DOMAIN_ALLOWED_EDGES.some(
            (e) => e.file === f.rel && e.domain === hit.captured,
          )
          if (allowed) continue
          problems.push(
            `✗ 反向依赖：${w.path}（${w.layer}：${w.note}）引用域目录 ${hit.captured} → ` +
              `${f.rel}:${hit.line}（${hit.match}）\n` +
              `    ${hit.text}\n` +
              `    分层规则：壳 → 域 → 基础设施；基础设施→域业务边归零且可机械守门` +
              `（ADR-0071 决策 6，#538）；设计意图边须逐条留痕于本脚本 ` +
              `INFRA_DOMAIN_ALLOWED_EDGES（附 ADR 指针），或把逻辑下沉域目录`,
          )
        }
      }
      if (isBusinessDomain) {
        for (const hit of scanSyncEngineRefs(source)) {
          problems.push(
            `✗ 业务域引用同步域内部件：${f.rel}:${hit.line}（${hit.match}）\n` +
              `    ${hit.text}\n` +
              `    严形态（ADR-0101 决策 4b）：业务域引用同步域合法面 = 契约模块 ` +
              `command ∪ 根再导出白名单 {DomainCommand, record_local, device_id}；` +
              `内部模块路径（engine::/ops::/model::…）一律红——` +
              `「重放不产本地 op」从结构巧合升为规格`,
          )
        }
      }
    }
  }
  return scanned
}

function main(): void {
  const repoRoot = fileURLToPath(new URL('..', import.meta.url))
  const srcDir = process.argv[2] ?? join(repoRoot, 'src-tauri', 'src')
  const srcTauriDir = process.argv[3] ?? join(repoRoot, 'src-tauri')
  const problems: string[] = []
  let scannedFiles = 0
  const domainCount = WHITELIST.filter((w) => w.layer === LAYER.DOMAIN).length

  // 模型域化禁令（规则①/②）+ 原生事务语句禁令（规则③）：全树扫描（壳、域、
  // 基础设施、顶层文件），残留引用可出现在任何层；collectRustFiles 自带测试
  // 豁免（ADR-0056 决策 5）——外挂测试目录的直置事务边界合法。
  // srcDir 整体不可达时静默交由白名单循环报「路径不存在」，不在此抛栈。
  let allFiles: RustFileRef[] = []
  try {
    allFiles = [
      ...collectRustFiles(srcDir, ''),
      ...collectRustFiles(join(srcTauriDir, INFRA_SRC_REL), INFRA_SRC_REL),
    ]
  } catch {
    // 目录缺失：白名单循环会逐条报错并 fail loud
  }
  for (const f of allFiles) {
    const source = readFileSync(f.abs, 'utf8')
    for (const hit of scanRustSource(source, GLOBAL_MODEL_PATH_PATTERN)) {
      problems.push(
        `✗ 全局模型路径残留：${f.rel}:${hit.line}（${hit.match}）\n` +
          `    ${hit.text}\n` +
          `    全局模型目录已随 ADR-0059 模型域化消亡（T7 / #424），` +
          `模型类型一律走域路径显式 import（如 crate::transaction::model::Transaction）` +
          `——防扁平命名空间复活`,
      )
    }
    for (const hit of scanRustSource(source, MODEL_GLOB_REEXPORT_PATTERN)) {
      problems.push(
        `✗ 域模型 glob 再导出：${f.rel}:${hit.line}（${hit.match}）\n` +
          `    ${hit.text}\n` +
          `    域 model 只许逐类型再导出，所有权必须逐类型可见 ` +
          `（ADR-0059 决策 3/6，#424）：改为 pub use model::{TypeA, TypeB} 形态`,
      )
    }
    if (isModelFile(f.rel)) {
      for (const hit of scanRustSource(source, MODEL_FILE_GLOB_PATTERN)) {
        problems.push(
          `✗ 域模型文件内 glob 聚合：${f.rel}:${hit.line}（${hit.match}）\n` +
            `    ${hit.text}\n` +
            `    域模型文件只承载本域类型定义与逐类型再导出，禁止 glob 聚合 ` +
            `（ADR-0059 决策 3/6，#424）`,
        )
      }
    }
    for (const hit of scanRustSource(source, NATIVE_TX_STMT_PATTERN, true)) {
      if (f.rel === NATIVE_TX_STMT_ALLOWED) continue
      problems.push(
        `✗ 原生事务语句：${f.rel}:${hit.line}（${hit.match}）\n` +
          `    ${hit.text}\n` +
          `    事务壳归基础设施 db::tx_scope（无条件自持 hold_transaction / ` +
          `嵌套感知 ensure_transaction，ADR-0056 / ADR-0105；#1013/#1014）——` +
          `产品代码不得手写 BEGIN/COMMIT/ROLLBACK，唯一合法住址 ${NATIVE_TX_STMT_ALLOWED}`,
      )
    }
  }

  // 模块清单核对：域目录相对根 src 扫描（壳层反向依赖 + 业务域→同步域严形态），
  // 基础设施模块相对 `crates/infra/src` 扫描（壳层反向依赖 + 基础设施→域认许边）——
  // 自 #1088 全量归位起基础设施不再住根 src，路径基准随归位改一次、事实源仍只有
  // 本脚本一份（ADR-0056「白名单即规格」不变）。
  scannedFiles += scanModuleEntries(WHITELIST, srcDir, problems)
  scannedFiles += scanModuleEntries(INFRA_MODULES, join(srcTauriDir, INFRA_SRC_REL), problems)

  if (scannedFiles === 0) {
    problems.push('✗ 全部白名单条目扫不到任何非测试 Rust 文件——src 目录指错或白名单整体漂移，拒绝以空集假绿通过')
  }

  // crate 边界核对（spec #1086 / issue #1087）：成员登记、门禁继承、依赖方向、
  // 静态检查/测试命令的 workspace 覆盖——与模块路径白名单并列，同为删除即变红。
  problems.push(...checkCrateBoundaries(srcTauriDir))

  if (problems.length > 0) {
    for (const p of problems) console.error(p)
    console.error(
      `❌ 结构守门失败：${problems.length} 处问题` +
        `（分层规则：壳 → 域 → 基础设施，域永不依赖壳，白名单即规格，见 ADR-0056）`,
    )
    process.exit(1)
  }
  console.log(
    `✓ 结构守门：白名单 ${WHITELIST.length} 项（域目录 ${domainCount}）` +
      `+ 基础设施模块 ${INFRA_MODULES.length} 项（crate ${INFRA_SRC_REL}）` +
      `· 白名单面非测试文件 ${scannedFiles} 个 · 对壳层零依赖` +
      `· 基础设施→域零未认许引用（认许边 ${INFRA_DOMAIN_ALLOWED_EDGES.length} 条，ADR-0071）` +
      `· 业务域→同步域严形态零违规（契约模块 ${SYNC_CONTRACT_MODULE} ∪ 白名单 ${SYNC_ROOT_ALLOWED_SYMBOLS.size} 符号，ADR-0101）` +
      `· 模型域化禁令全树扫描 ${allFiles.length} 个文件零残留（ADR-0059）` +
      `· 原生事务语句全树扫描 ${allFiles.length} 个文件仅 ${NATIVE_TX_STMT_ALLOWED} 一处（#1014）` +
      `· crate 边界 ${CRATES.length} 个（成员登记 / 门禁继承 / 依赖方向 / workspace 命令覆盖，#1087）`,
  )
}

// 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的扫描函数。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
