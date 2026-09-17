import { afterAll, describe, expect, it } from 'vitest'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  ACCOUNTS_MODULES,
  ACCOUNTS_SRC_REL,
  BACKUP_MODULES,
  BACKUP_SRC_REL,
  BUDGET_MODULES,
  BUDGET_SRC_REL,
  CATEGORIES_MODULES,
  CATEGORIES_SRC_REL,
  CRATES,
  CURRENCIES_MODULES,
  CURRENCIES_SRC_REL,
  DASHBOARD_MODULES,
  DASHBOARD_SRC_REL,
  DOMAIN_PAIR_ALLOWED_EDGES,
  DOMAIN_PAIR_FORBIDDEN,
  INFRA_MODULES,
  INFRA_SRC_REL,
  INVESTMENT_MODULES,
  INVESTMENT_SRC_REL,
  ITEM_MODULES,
  ITEM_SRC_REL,
  MARKET_SYNC_MODULES,
  MARKET_SYNC_SRC_REL,
  MERCHANTS_MODULES,
  MERCHANTS_SRC_REL,
  POLICY_MODULES,
  POLICY_SRC_REL,
  PHYSICAL_ASSET_MODULES,
  PHYSICAL_ASSET_SRC_REL,
  PROTOCOL_MODULES,
  PROTOCOL_SRC_REL,
  REPORTS_MODULES,
  REPORTS_SRC_REL,
  SCHEDULED_MODULES,
  SCHEDULED_SRC_REL,
  SYNC_ENGINE_MODULES,
  SYNC_ENGINE_SRC_REL,
  TRANSACTION_MODULES,
  TRANSACTION_SRC_REL,
  TRANSACTION_ZONE_ALLOWED_EDGES,
  WHITELIST,
  LAYER,
  maskNonCode,
} from '../scripts/check-structure.ts'
import { gateScript, runGateScript } from './run-gate-script.test-helper.ts'

// 被测对象是仓库工具脚本 scripts/check-structure.ts（结构守门，ADR-0056）。
// 脚本以 Bun 运行时执行（ADR-0083）：runGateScript 以 spawnSync('bun') 与门槛
// 调用同款拉起，测的就是门槛路径。
// 按测试决策只测外部可观察结果——进程退出码与输出，不测内部函数；
// 通过位置参数把扫描目标指向临时夹具目录。
// 夹具白名单清单自脚本导出的 WHITELIST 派生（单一事实源，无双源漂移）。
const script = gateScript('check-structure.ts')
const run = (args: string[]) => runGateScript(script, args)

const tempDirs: string[] = []
afterAll(() => {
  for (const dir of tempDirs) rmSync(dir, { recursive: true, force: true })
})

/** 白名单条目桩内容（无壳层依赖的最小 Rust 文件） */
const STUB = '// 结构守门夹具桩\npub fn stub() {}\n'

/**
 * 按脚本导出的清单写桩（目录 → mod.rs，文件 → 同名文件）：域目录条目落
 * `<srcTauri>/src`，基础设施模块条目落 `<srcTauri>/crates/infra/src`（#1088 起
 * 基础设施整体住 crate）。
 */
function writeModuleStubs(baseDir: string, entries: readonly { path: string }[]): void {
  for (const { path } of entries) {
    const abs = join(baseDir, path)
    if (path.endsWith('.rs')) {
      mkdirSync(join(abs, '..'), { recursive: true })
      writeFileSync(abs, STUB)
    } else {
      mkdirSync(abs, { recursive: true })
      writeFileSync(join(abs, 'mod.rs'), STUB)
    }
  }
}

function populateWhitelistEntries(srcTauri: string): void {
  writeModuleStubs(join(srcTauri, 'src'), WHITELIST)
  writeModuleStubs(join(srcTauri, INFRA_SRC_REL), INFRA_MODULES)
  writeModuleStubs(join(srcTauri, PROTOCOL_SRC_REL), PROTOCOL_MODULES)
  writeModuleStubs(join(srcTauri, BACKUP_SRC_REL), BACKUP_MODULES)
  writeModuleStubs(join(srcTauri, TRANSACTION_SRC_REL), TRANSACTION_MODULES)
  writeModuleStubs(join(srcTauri, ACCOUNTS_SRC_REL), ACCOUNTS_MODULES)
  writeModuleStubs(join(srcTauri, CATEGORIES_SRC_REL), CATEGORIES_MODULES)
  writeModuleStubs(join(srcTauri, MERCHANTS_SRC_REL), MERCHANTS_MODULES)
  writeModuleStubs(join(srcTauri, CURRENCIES_SRC_REL), CURRENCIES_MODULES)
  writeModuleStubs(join(srcTauri, POLICY_SRC_REL), POLICY_MODULES)
  writeModuleStubs(join(srcTauri, SCHEDULED_SRC_REL), SCHEDULED_MODULES)
  writeModuleStubs(join(srcTauri, BUDGET_SRC_REL), BUDGET_MODULES)
  writeModuleStubs(join(srcTauri, PHYSICAL_ASSET_SRC_REL), PHYSICAL_ASSET_MODULES)
  writeModuleStubs(join(srcTauri, REPORTS_SRC_REL), REPORTS_MODULES)
  writeModuleStubs(join(srcTauri, ITEM_SRC_REL), ITEM_MODULES)
  writeModuleStubs(join(srcTauri, INVESTMENT_SRC_REL), INVESTMENT_MODULES)
  writeModuleStubs(join(srcTauri, DASHBOARD_SRC_REL), DASHBOARD_MODULES)
  writeModuleStubs(join(srcTauri, MARKET_SYNC_SRC_REL), MARKET_SYNC_MODULES)
  writeModuleStubs(join(srcTauri, SYNC_ENGINE_SRC_REL), SYNC_ENGINE_MODULES)
}

/** 基础设施模块路径判定（覆盖文件按此归位：命中即落 crate，其余落根 src）。 */
const INFRA_ENTRY_PATHS = new Set(INFRA_MODULES.map((m) => m.path))

function isInfraModulePath(rel: string): boolean {
  const head = rel.split('/')[0]
  return INFRA_ENTRY_PATHS.has(head) || INFRA_ENTRY_PATHS.has(rel)
}

/** 备份域 crate 模块路径判定（精确文件名；与基础设施清单无交集，#1091）。 */
const BACKUP_ENTRY_PATHS = new Set(BACKUP_MODULES.map((m) => m.path))

function isBackupModulePath(rel: string): boolean {
  return BACKUP_ENTRY_PATHS.has(rel)
}

/** 核心交易域 crate 模块路径判定（清单条目或其子路径，#1092/#1182 四区目录化）。 */
const TRANSACTION_ENTRY_PATHS = new Set(TRANSACTION_MODULES.map((m) => m.path))

function isTransactionModulePath(rel: string): boolean {
  const head = rel.split('/')[0]
  return TRANSACTION_ENTRY_PATHS.has(head) || TRANSACTION_ENTRY_PATHS.has(rel)
}

/** 账户域 crate 模块路径判定（精确文件名，#1093；与基础设施清单无交集）。 */
const ACCOUNTS_ENTRY_PATHS = new Set(ACCOUNTS_MODULES.map((m) => m.path))

function isAccountsModulePath(rel: string): boolean {
  return ACCOUNTS_ENTRY_PATHS.has(rel)
}

/** 分类域 crate 模块路径判定（精确文件名，#1094；排在交易域之后——`command.rs`
 *  / `model.rs` 两名与交易域清单撞名，撞名条目优先落交易域，夹具对分类域只用
 *  无撞名的 `core.rs`）。 */
const CATEGORIES_ENTRY_PATHS = new Set(CATEGORIES_MODULES.map((m) => m.path))

function isCategoriesModulePath(rel: string): boolean {
  return CATEGORIES_ENTRY_PATHS.has(rel)
}

/** 商户域 crate 模块路径判定（精确文件名，#1096；与基础设施/交易清单无交集的
 *  仅 crud.rs——command.rs / model.rs 与交易清单同名，路由优先级归交易 crate）。 */
const MERCHANTS_ENTRY_PATHS = new Set(MERCHANTS_MODULES.map((m) => m.path))

function isMerchantsModulePath(rel: string): boolean {
  return MERCHANTS_ENTRY_PATHS.has(rel)
}

/** 币种域 crate 模块路径判定（精确文件名，#1095；与基础设施/交易域清单无交集）。 */
const CURRENCIES_ENTRY_PATHS = new Set(CURRENCIES_MODULES.map((m) => m.path))

function isCurrenciesModulePath(rel: string): boolean {
  return CURRENCIES_ENTRY_PATHS.has(rel)
}

/** 保单域 crate 模块路径判定（精确文件名，#1100；`command.rs` / `model.rs` 与
 *  交易域清单、`crud.rs` 与商户域清单撞名，路由优先级归先登记的 crate，夹具
 *  对保单域只用无撞名的 `insurer.rs` / `stats.rs` / `validation.rs`）。 */
const POLICY_ENTRY_PATHS = new Set(POLICY_MODULES.map((m) => m.path))

function isPolicyModulePath(rel: string): boolean {
  return POLICY_ENTRY_PATHS.has(rel)
}

/** 定时计划域 crate 模块路径判定（精确文件名，#1098；无撞名可用——`engine.rs`
 *  归备份域、`command.rs` 归账户域，均在前序链先行占有；定时计划夹具只用
 *  `auto_run.rs` / `models.rs` / `source.rs` / `spend.rs` 四名）。 */
const SCHEDULED_ENTRY_PATHS = new Set(SCHEDULED_MODULES.map((m) => m.path))

function isScheduledModulePath(rel: string): boolean {
  return SCHEDULED_ENTRY_PATHS.has(rel)
}

/** 预算域 crate 模块路径判定（精确文件名，#1101；`command.rs` / `model.rs` 与
 *  交易域清单、`crud.rs` 与商户域清单撞名，路由优先级归先登记的 crate，夹具
 *  对预算域只用无撞名的 `progress.rs`）。 */
const BUDGET_ENTRY_PATHS = new Set(BUDGET_MODULES.map((m) => m.path))

function isBudgetModulePath(rel: string): boolean {
  return BUDGET_ENTRY_PATHS.has(rel)
}

/** 物品域 crate 模块路径判定（精确文件名，#1099；与基础设施/交易清单无交集的
 *  为 cost.rs / domain.rs / guard.rs——command.rs / model.rs 与交易清单同名，
 *  路由优先级归交易 crate）。 */
const ITEM_ENTRY_PATHS = new Set(ITEM_MODULES.map((m) => m.path))

function isItemModulePath(rel: string): boolean {
  return ITEM_ENTRY_PATHS.has(rel)
}

/** 投资域 crate 模块路径判定（精确文件名，#1097；撞名条目优先落先登记 crate，
 *  夹具对投资域只用无撞名条目 trend.rs / holdings.rs / channel.rs）。 */
const INVESTMENT_ENTRY_PATHS = new Set(INVESTMENT_MODULES.map((m) => m.path))

function isInvestmentModulePath(rel: string): boolean {
  return INVESTMENT_ENTRY_PATHS.has(rel)
}

/** 行情同步域 crate 模块路径判定（精确文件名，#1106；撞名条目优先落先登记 crate，
 *  夹具对行情同步域只用无撞名条目 http.rs / incremental.rs / fund_nav.rs /
 *  persist.rs——model.rs / progress.rs / fund.rs / stock.rs 已被先登记 crate 占有）。 */
const MARKET_SYNC_ENTRY_PATHS = new Set(MARKET_SYNC_MODULES.map((m) => m.path))

function isMarketSyncModulePath(rel: string): boolean {
  return MARKET_SYNC_ENTRY_PATHS.has(rel)
}

/** 多端同步域 crate 模块路径判定（精确文件名/目录，#1107；撞名条目优先落先登记
 *  crate，夹具对同步域只用无撞名条目 `checkpoint.rs` / `envelope.rs`） */
const SYNC_ENGINE_ENTRY_PATHS = new Set(SYNC_ENGINE_MODULES.map((m) => m.path))

function isSyncEngineModulePath(rel: string): boolean {
  const head = rel.split('/')[0]
  return SYNC_ENGINE_ENTRY_PATHS.has(head) || SYNC_ENGINE_ENTRY_PATHS.has(rel)
}

/**
 * 写覆盖文件：按路径首段归位——基础设施模块（`db/…` / `error.rs` / …）落
 * `<srcTauri>/crates/infra/src`，备份域 crate 模块（`auto.rs` / `engine.rs`，#1091）
 * 落 `<srcTauri>/crates/backup/src`，核心交易域 crate 模块（#1092）与账户域 crate
 * 模块（#1093）、分类域 crate 模块（#1094）、商户域 crate 模块（#1096）、币种域
 * crate 模块（#1095）、物品域 crate 模块（#1099）各自落同名 crate，其余（域目录、
 * 壳层 `commands/` 等）落 `<srcTauri>/src`。行情同步域 crate 模块（#1106）排在
 * 投资域之后判定（撞名条目优先归先登记的 crate，夹具只用无撞名条目 `http.rs` /
 * `incremental.rs`）。账户域与分类域在交易域之后判定：
 * `model.rs` / `command.rs` 两名为交易域清单先行占有（与 placeOverride 的先后链
 * 一致；商户域撞名同理，仅 `crud.rs` 归商户 crate；物品域撞名同理，仅
 * `cost.rs` / `domain.rs` / `guard.rs` 归物品 crate；投资域撞名同理，夹具只用
 * `trend.rs` / `holdings.rs` / `channel.rs` 等无撞名条目）。
 */
function placeOverride(srcTauri: string, relPath: string, content: string): void {
  const base = isInfraModulePath(relPath)
    ? join(srcTauri, INFRA_SRC_REL)
    : isBackupModulePath(relPath)
      ? join(srcTauri, BACKUP_SRC_REL)
      : isTransactionModulePath(relPath)
        ? join(srcTauri, TRANSACTION_SRC_REL)
        : isAccountsModulePath(relPath)
          ? join(srcTauri, ACCOUNTS_SRC_REL)
        : isCategoriesModulePath(relPath)
          ? join(srcTauri, CATEGORIES_SRC_REL)
        : isMerchantsModulePath(relPath)
          ? join(srcTauri, MERCHANTS_SRC_REL)
        : isCurrenciesModulePath(relPath)
          ? join(srcTauri, CURRENCIES_SRC_REL)
        : isPolicyModulePath(relPath)
          ? join(srcTauri, POLICY_SRC_REL)
        : isScheduledModulePath(relPath)
          ? join(srcTauri, SCHEDULED_SRC_REL)
        : isBudgetModulePath(relPath)
          ? join(srcTauri, BUDGET_SRC_REL)
        : isItemModulePath(relPath)
          ? join(srcTauri, ITEM_SRC_REL)
        : isInvestmentModulePath(relPath)
          ? join(srcTauri, INVESTMENT_SRC_REL)
        : isMarketSyncModulePath(relPath)
          ? join(srcTauri, MARKET_SYNC_SRC_REL)
        : isSyncEngineModulePath(relPath)
          ? join(srcTauri, SYNC_ENGINE_SRC_REL)
          : join(srcTauri, 'src')
  const file = join(base, relPath)
  mkdirSync(join(file, '..'), { recursive: true })
  writeFileSync(file, content)
}

/**
 * 建临时夹具：按脚本导出的 WHITELIST 生成全部条目，
 * 再按 overrides 追加/覆盖文件。返回脚本参数（夹具 src 目录）。
 */
function makeFixture(overrides: Record<string, string> = {}): string[] {
  const args = makeCrateFixture()
  for (const [relPath, content] of Object.entries(overrides)) {
    placeOverride(args[1], relPath, content)
  }
  return args
}

// 夹具用现存的壳层引用形态（商户壳层命令）：参考数据三域 #404 归位后账户壳层已无
// `*_internal` 下沉函数，夹具文本取现存壳层命令与实际结构保持一致。
const shellUse = 'use crate::commands::merchants::list_merchants;\npub fn x() {}\n'

describe('check-structure（结构守门）', () => {
  it('真实仓库默认通过：白名单对壳层零依赖', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain('零依赖')
    // 摘要中的域目录数自脚本导出的 WHITELIST 派生（单一事实源，迁域追加白名单后不再漂移）
    const domainCount = WHITELIST.filter((w) => w.layer === LAYER.DOMAIN).length
    expect(r.output).toContain(`域目录 ${domainCount}`)
  })

  it('夹具全部为干净桩时通过', () => {
    const args = makeFixture()
    const r = run(args)
    expect(r.status).toBe(0)
    expect(r.output).toContain('零依赖')
  })

  it('域目录代码引用壳层 → 失败并定位文件行号', () => {
    // 全部业务域 crate 化后，根包仅余测试支持域（test_support），域目录壳层
    // 反向依赖靶随之更替；业务域 crate 的反向依赖由各自模块清单用例覆盖。
    const args = makeFixture({ 'test_support/crud.rs': shellUse })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('反向依赖')
    expect(r.output).toContain('test_support/crud.rs:1')
  })

  it('基础设施 crate 内代码引用壳层 → 失败并定位文件行号（#1088 归位后清单基准在 crate）', () => {
    const args = makeFixture({ 'db/helper.rs': shellUse })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('反向依赖')
    expect(r.output).toContain('db/helper.rs:1')
  })

  it('注释与字符串中的 commands:: 不误报（掩码边界）', () => {
    const args = makeFixture({
      'test_support/cost.rs': [
        '/// 消费 `commands::item` 接缝（文档注释不算依赖）',
        '// 见 commands::foo 说明',
        'let url = "http://127.0.0.1:9527/commands::x";',
        'let re = r#"commands::\\d+"#;',
        "pub fn f<'a>(x: &'a str) -> &str { x }",
        '',
      ].join('\n'),
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('外挂测试模块/目录豁免：tests.rs 与 tests/ 引用壳层不红（ADR-0056 决策 5）', () => {
    const args = makeFixture({
      'test_support/tests.rs': shellUse,
      'test_support/tests/scaffold.rs': shellUse,
      'test_support/helper/tests/fixture.rs': shellUse,
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('别名引入（use … as）同样识别为依赖', () => {
    const args = makeFixture({ 'db/helper.rs': 'use crate::commands as shell;\npub fn y() {}\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('commands as')
  })

  it('白名单路径缺失（清单漂移）→ fail loud', () => {
    const args = makeFixture()
    rmSync(join(args[0], 'test_support'), { recursive: true, force: true })
    const r = run(args)
    expect(r.status).toBe(1)
    // 断言对准缺失条目的路径字段（留痕 note 里也含 test_support，裸域名断言偏弱）。
    expect(r.output).toContain('白名单路径不存在：test_support')
  })

  it('基础设施模块清单路径缺失（crate 内清单漂移）→ fail loud', () => {
    const args = makeFixture()
    rmSync(join(args[1], INFRA_SRC_REL, 'db'), { recursive: true, force: true })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('白名单路径不存在')
    expect(r.output).toContain('db')
  })

  it('白名单条目只剩测试豁免文件（扫不到非测试文件）→ fail loud，拒绝假绿', () => {
    const args = makeFixture()
    rmSync(join(args[0], 'test_support', 'mod.rs'))
    writeFileSync(join(args[0], 'test_support', 'tests.rs'), shellUse) // 只剩豁免形态
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('扫不到非测试')
  })
})

describe('check-structure 基础设施→域扫描（ADR-0071 决策 6 / #538）', () => {
  /**
   * 迁移前 db/mod.rs after_commit 的形态（ADR-0032 置脏单点）：#1088 起该生产边
   * 已由注册点反转消除（基础设施只留调用时机、备份域提供实现），故本形态现为
   * **未认许**的产出式反向引用——夹具用它钉死「生产挂载点不得复活」。
   */
  // 全部业务域 crate 化后，根包仅余测试支持域（test_support）；基础设施对该域
  // 的 4 条认许边全是测试专用边（ADR-0084），其余文件/目标仍红——「基础设施
  // 生产路径直调域副作用不得复活」的钉子不变；对业务域 crate 的依赖由 cargo
  // 依赖图（infra Cargo.toml 无域依赖）编译期拒绝。
  const afterCommitShape = [
    'pub fn write<T>(f: impl FnOnce() -> T) -> T { f() }',
    'fn after_commit(conn: &Connection) {',
    '    if let Err(e) = crate::test_support::mark_dirty(conn) {',
    '        tracing::warn!(error = %e, "写库成功但置脏失败（忽略）");',
    '    }',
    '    let dir = crate::test_support::shared_prefs().snapshot_dir();',
    '    crate::test_support::run_due_backup(',
    '        conn,',
    '        dir.as_deref(),',
    '    );',
    '}',
    '',
  ].join('\n')

  it('基础设施文件 use 域模块 → 红', () => {
    const args = makeFixture({ 'db/helper.rs': 'use crate::test_support::open;\npub fn x() {}\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('引用域目录')
    expect(r.output).toContain('db/helper.rs:1')
  })

  it('内联全限定路径（crate::域::x() 形态）同样识别 → 红', () => {
    const args = makeFixture({
      'db/helper.rs': 'pub fn y() { crate::test_support::open(); }\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('引用域目录 test_support')
    expect(r.output).toContain('db/helper.rs:1')
  })

  it('tauri_app_lib:: 前缀与 use as 别名引入同样识别 → 红', () => {
    // 全部业务域 crate 化后，根包仅余测试支持域；前缀与别名形态以非认许文件钉住。
    const args = makeFixture({
      'events.rs': 'use tauri_app_lib::test_support::open;\npub fn x() {}\n',
      'db/helper.rs': 'use crate::test_support as support;\npub fn y() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('引用域目录 test_support')
    expect(r.output).toContain('引用域目录 test_support')
  })

  it('模块自身导入（use crate::<域>;）同样识别 → 红', () => {
    const args = makeFixture({ 'db/helper.rs': 'use crate::test_support;\npub fn z() {}\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('引用域目录 test_support')
  })

  it('std::sync 等同名路径不误报（crate 根前缀限定边界）', () => {
    const args = makeFixture({
      'db/helper.rs': 'use std::sync::{Arc, Mutex};\npub fn z(a: Arc<Mutex<u8>>) {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('非禁边对的域间横向引用不红（扫描范围仅基础设施条目，ADR-0071 决策 5）', () => {
    // transaction→accounts 自 #1090 起属域间禁边（另见域间禁边 describe）；
    // 本用例改用非禁边对（保单域→核心交易域：真实且受 AC 允许的域→域上层依赖，
    // dashboard 靶随 #1104 crate 化退役、sync 靶随 #1106 crate 化更替）钉住
    // 「域间横向引用本身不在 infra 扫描范围」——'insurer.rs' 经 placeOverride 落
    // 保单域 crate（#1100 后守门基准随迁 crate，业务域扫描照扫）。
    const args = makeFixture({
      'insurer.rs': 'use ledger_transaction::amount::TransactionKind;\npub fn x() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('注释与字符串中的域路径不误报（掩码边界）', () => {
    const args = makeFixture({
      'db/helper.rs': [
        '/// 提交点由 [`crate::backup::run_due_backup`] 统一门禁（文档注释不算引用）',
        '// 见 crate::test_support::open 说明',
        'let s = "crate::backup::mark_dirty";',
        'let re = r#"crate::test_support::open"#;',
        'pub fn f() {}',
        '',
      ].join('\n'),
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('外挂测试豁免不变：tests.rs 与 tests/ 目录引用域不红（ADR-0056 决策 5）', () => {
    const args = makeFixture({
      'db/tests.rs': 'use crate::test_support::open;\n',
      'db/tests/common.rs': 'pub fn s() -> crate::backup::AutoBackupState { todo!() }\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('生产挂载点已反转：db/mod.rs 直调域副作用 → 红（认许边不再含该条）', () => {
    const args = makeFixture({ 'db/mod.rs': afterCommitShape })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('引用域目录 test_support')
    expect(r.output).toContain('db/mod.rs:3')
  })

  it('认许边精确匹配：settings.rs→test_support 绿；他文件同域仍红', () => {
    const green = makeFixture({
      'settings.rs': 'use tauri_app_lib::test_support::open;\npub fn x() {}\n',
    })
    expect(run(green).status).toBe(0)

    const otherFile = makeFixture({
      'db/helper.rs': 'use crate::test_support::open;\n',
    })
    const r2 = run(otherFile)
    expect(r2.status).toBe(1)
    expect(r2.output).toContain('db/helper.rs:1')
  })

  it('真实仓库默认通过：基础设施→域零未认许引用（认许边留痕于脚本）', () => {
    const r = run([])
    expect(r.status).toBe(0)
    // 生产挂载点 0（#1088 注册点反转消除 db/mod.rs→backup）+ settings.rs→test_support
    //（ADR-0084，#758）；shell_support 三条测试专用边随 #1108 迁出根包退役。
    expect(r.output).toContain('认许边 1 条')
  })
})

describe('check-structure 业务域→同步域零容忍（ADR-0101 决策 4b / #1089 收紧）', () => {
  it('业务域引用同步域内部件（engine::/ops::/model::…）→ 红并定位文件行号', () => {
    // 'auto_run.rs' 经 placeOverride 落定时计划域 crate（SCHEDULED_MODULES 派生
    // 路由，#1098 起守门基准随迁 crate；原 scheduled_transactions/command.rs
    // 域目录夹具随拆分消亡）。
    const args = makeFixture({
      'auto_run.rs': 'use crate::sync_engine::engine::ReplayEffect;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('auto_run.rs:1')
    expect(r.output).toContain('sync_engine::engine')
  })

  it('契约模块与原白名单根符号亦红（#1089 零容忍：协议面下放协议 crate）', () => {
    // 'insurer.rs' 经 placeOverride 落保单域 crate（POLICY_MODULES 派生路由）——
    // 业务域靶随 #1106 行情同步域拆出改以仍在 crate 清单的保单域承载（原
    // `sync/command.rs` 域目录夹具随拆分消亡）。
    const args = makeFixture({
      'insurer.rs': [
        'use crate::sync_engine::command::ReplayEffect;',
        'use crate::sync_engine::{DomainCommand, record_local as record_op};',
        'use crate::sync_engine::device_id;',
        'use crate::sync_engine;',
        'pub fn x() {}',
        '',
      ].join('\n'),
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('insurer.rs:1')
  })

  it('根花括号列举夹带任一符号 → 红（零容忍逐条判定）', () => {
    const args = makeFixture({
      'insurer.rs': 'use crate::sync_engine::{DomainCommand, model::SyncOp};\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('model')
  })

  it('根 glob 引入 → 红（零容忍）', () => {
    const args = makeFixture({ 'insurer.rs': 'use crate::sync_engine::*;\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
  })

  it('根别名引入（use crate::sync_engine as se）→ 红（堵别名盲区）', () => {
    const args = makeFixture({
      'insurer.rs': 'use crate::sync_engine as se;\npub fn x() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('insurer.rs:1')
  })

  it('业务域直接引用同步域 crate 名（ledger_sync_engine::）→ 红（#1107 crate 化后堵漏）', () => {
    const args = makeFixture({
      'insurer.rs': 'use ledger_sync_engine::engine::ReplayEffect;\npub fn x() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('ledger_sync_engine::engine')
  })

  it('同步域自身与测试支持域不参与（作用域边界）', () => {
    const args = makeFixture({
      // checkpoint.rs 经 placeOverride 落多端同步域 crate（SYNC_ENGINE_MODULES
      // 派生路由，#1107）；同步域自身不参与业务域→同步域零容忍扫描。
      'checkpoint.rs': 'use crate::sync_engine::ops::insert_row;\n',
      'test_support/channel.rs': 'use crate::sync_engine::model::SyncOp;\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('注释与字符串中的同步域内部路径不误报（掩码边界）', () => {
    const args = makeFixture({
      'insurer.rs': [
        '/// 见 `crate::sync_engine::ops::record_local` 说明（文档注释不算引用）',
        '// crate::sync_engine::engine::dispatch',
        'let s = "crate::sync_engine::parked::ParkedOp";',
        'let re = r#"crate::sync_engine::model::SyncOp"#;',
        'pub fn f() {}',
        '',
      ].join('\n'),
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('真实仓库默认通过：业务域→同步域零容忍零违规', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain('业务域→同步域零容忍零违规')
  })
})

describe('check-structure 域间禁边（issue #1090 写路径副作用接缝反转）', () => {
  it('transaction 起点禁边已随 crate 化退役（#1092）：文本清单不再辖，依赖方向归 cargo 依赖图', () => {
    // 核心交易域拆为 ledger-transaction crate 后，对业务域/壳层的引用由生产依赖面
    // 编译期拒绝；带类型签名的钩子无法跨实例（名义类型不等价），文本扫描对 crate
    // 内代码不再可及，故 from='transaction' 规则全部退役（守门基准随迁
    // TRANSACTION_MODULES：对壳层/同步域零容忍照扫）。
    expect(DOMAIN_PAIR_FORBIDDEN.some((r) => r.from === 'transaction')).toBe(false)
  })

  it('scheduled_transactions→backup 禁边已随 crate 化退役（#1098）：文本清单不再辖，依赖方向归 cargo 依赖图', () => {
    // 定时计划域拆为 ledger-scheduled crate 后：置脏实现住 ledger-backup、追补触发
    // 实现住本域，双向均经注册点接缝、壳层对装；本域生产依赖面不含 ledger-backup，
    // 构造引用即编译失败（守门基准随迁 SCHEDULED_MODULES：对壳层/同步域零容忍照
    // 扫），故最后一条规则退役、清单归空（保留空集留痕）。
    expect(DOMAIN_PAIR_FORBIDDEN.some((r) => r.from === 'scheduled_transactions')).toBe(false)
    expect(DOMAIN_PAIR_FORBIDDEN).toHaveLength(0)
  })

  it('核心交易域 crate 模块引用壳层 → 红并定位文件行号（#1092 crate 化后守门基准随迁）', () => {
    // 'write/writer.rs' 经 placeOverride 落核心交易域 crate（TRANSACTION_MODULES 派生路由）。
    const args = makeFixture({ 'write/writer.rs': shellUse })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('反向依赖')
    expect(r.output).toContain('write/writer.rs:1')
  })

  it('核心交易域 crate 模块引用同步域 → 红（业务域→同步域零容忍覆盖 crate，#1092）', () => {
    const args = makeFixture({
      'write/protocol.rs': 'use tauri_app_lib::sync_engine::registry::dispatch;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('write/protocol.rs:1')
  })

  it('方向性：账户域 crate 引用核心交易域 crate（生产依赖方向合法）不红', () => {
    // #1093 起账户域为独立 crate：accounts → transaction 是 Cargo.toml 声明的合法
    // 生产依赖（余额口径消费 kind→度量矩阵，ADR-0071 决策 5 修订后方向）。
    // 'core.rs' 经 placeOverride 落账户域 crate（ACCOUNTS_MODULES 派生路由）。
    const args = makeFixture({
      'core.rs': 'use ledger_transaction::amount::account_flow_expr;\npub fn x() {}\n',
    })
    expect(run(args).status).toBe(0)
  })

  it('注释与字符串中的禁边路径不误报（掩码边界）', () => {
    const args = makeFixture({
      'transaction/writer.rs': [
        '/// 经接缝（#1090）替代旧 `crate::accounts::balance` 直引（文档注释不算依赖）',
        '// 见 crate::scheduled_transactions::source 说明',
        'let s = "crate::backup::mark_dirty";',
        'pub fn f() {}',
        '',
      ].join('\n'),
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('外挂测试豁免不变：tests/ 目录引用禁边对不红（ADR-0056 决策 5）', () => {
    const args = makeFixture({
      'transaction/tests/balance_cache.rs': 'use ledger_transaction::read::list_transactions;\n',
      'tests/auto_run.rs': 'crate::backup::get_state(&conn);\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('真实仓库默认通过：域间禁边零未认许引用（认许边留痕于脚本）', () => {
    const r = run([])
    expect(r.status).toBe(0)
    // 汇总字符串自脚本导出清单派生（单一事实源，无双源漂移）。
    expect(r.output).toContain(
      `域间禁边 ${DOMAIN_PAIR_FORBIDDEN.length} 对零未认许引用`
        + `（认许边 ${DOMAIN_PAIR_ALLOWED_EDGES.length} 条，#1090 接缝反转）`,
    )
  })

})

describe('check-structure 模型域化禁令（ADR-0059 决策 6 / #424 T7 收口）', () => {
  it('规则①：crate::models 全局模型路径残留 → 红', () => {
    const args = makeFixture({
      'test_support/crud.rs': 'use crate::models::Transaction;\npub fn x() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('全局模型路径残留')
    expect(r.output).toContain('test_support/crud.rs:1')
  })

  it('规则①：tauri_app_lib::models 形态同样识别 → 红', () => {
    const args = makeFixture({
      'commands/transactions.rs': 'let t: tauri_app_lib::models::Transaction;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('全局模型路径残留')
  })

  it('规则①：注释与字符串中的 models 路径不误报（掩码边界）', () => {
    const args = makeFixture({
      'test_support/cost.rs': [
        '/// 全局模型目录已消亡，crate::models 是历史形态（文档注释不算引用）',
        '// 见 crate::models::Transaction 说明',
        'let s = "crate::models::Transaction";',
        'pub fn f() {}',
      ].join('\n'),
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('规则①：外挂测试豁免（tests.rs / tests/ 目录不参与扫描）', () => {
    const args = makeFixture({
      'test_support/tests.rs': 'use crate::models::Transaction;\n',
      'test_support/tests/scaffold.rs': 'use tauri_app_lib::models::Transaction;\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('规则②：域接缝 glob 再导出 pub use model::* → 红', () => {
    const args = makeFixture({ 'test_support/mod.rs': 'mod model;\npub use model::*;\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('glob 再导出')
    expect(r.output).toContain('test_support/mod.rs:2')
  })

  it('规则②：跨域拍平形态 pub use crate::x::model::* → 红', () => {
    const args = makeFixture({
      'test_support/mod.rs': 'pub use crate::transaction::model::*;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('glob 再导出')
  })

  it('规则②：旧全局目录同名形态 pub use models::* → 红', () => {
    const args = makeFixture({ 'test_support/mod.rs': 'pub use models::*;\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('glob 再导出')
  })

  it('规则②：域模型文件内 glob 聚合 pub use xxx::* → 红', () => {
    const args = makeFixture({ 'test_support/model.rs': 'pub use super::crud::*;\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('glob 聚合')
    expect(r.output).toContain('test_support/model.rs:1')
  })

  it('规则②：逐类型再导出与域内私有 glob 引用合规 → 绿', () => {
    const args = makeFixture({
      'test_support/mod.rs': 'mod model;\npub use model::{Item, ItemInput};\n',
      'test_support/behavior.rs': 'use super::model::*;\npub fn x() {}\n',
      'test_support/model.rs': 'pub struct Item;\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })
})

describe('check-structure 原生事务语句禁令（issue #1014 / #1003 定案 7）', () => {
  it('产品代码手写 BEGIN → 红并定位文件行号', () => {
    // 'core.rs' 经 placeOverride 落账户域 crate（ACCOUNTS_MODULES 派生路由，#1093），
    // 全树扫描覆盖 crate 内文件。
    const args = makeFixture({
      'core.rs': 'pub fn f(conn: &Connection) {\n    conn.execute("BEGIN", []);\n}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('原生事务语句')
    expect(r.output).toContain('core.rs:2')
    expect(r.output).toContain('db/tx_scope.rs')
  })

  it('COMMIT / ROLLBACK 手写同样识别 → 红', () => {
    // 'source.rs' 经 placeOverride 落定时计划域 crate（SCHEDULED_MODULES 派生路由，
    // #1098 起全树扫描覆盖 crate；原 scheduled_transactions/engine.rs 域目录夹具
    // 随拆分消亡）。
    const args = makeFixture({
      'transaction/batch.rs': 'pub fn g(conn: &Connection) {\n    conn.execute("COMMIT", []);\n}\n',
      'source.rs': 'pub fn h(conn: &Connection) {\n    conn.execute("ROLLBACK", []);\n}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('transaction/batch.rs:2')
    expect(r.output).toContain('source.rs:2')
  })

  it('唯一合法住址 db/tx_scope.rs 内的原生事务语句 → 绿', () => {
    const args = makeFixture({
      'db/tx_scope.rs': 'pub fn hold(conn: &Connection) {\n    conn.execute("BEGIN", []);\n}\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('注释中的 execute("BEGIN") 不误报（只掩码注释、保留字符串）', () => {
    const args = makeFixture({
      'core.rs': [
        '// 原生事务语句禁令：conn.execute("BEGIN", []) 只许出现在 db/tx_scope.rs',
        '/// conn.execute("COMMIT", [])',
        'pub fn f() {}',
        '',
      ].join('\n'),
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('真实仓库默认通过：原生事务语句仅 db/tx_scope.rs 一处', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain('原生事务语句全树扫描')
  })
})

/** workspace 骨架夹具的可覆盖面（缺省为一份全绿的骨架）。 */
interface CrateFixtureOverrides {
  rootManifest?: string
  memberManifest?: string
  /** 覆盖备份域 crate 的 `crates/backup/Cargo.toml`（依赖方向负向夹具，#1091） */
  backupManifest?: string
  /** 覆盖核心交易域 crate 的 `crates/transaction/Cargo.toml`（依赖方向负向夹具，#1092） */
  transactionManifest?: string
  /** 覆盖账户域 crate 的 `crates/accounts/Cargo.toml`（依赖方向负向夹具，#1093） */
  accountsManifest?: string
  /** 覆盖分类域 crate 的 `crates/categories/Cargo.toml`（依赖方向负向夹具，#1094） */
  categoriesManifest?: string
  /** 覆盖商户域 crate 的 `crates/merchants/Cargo.toml`（依赖方向负向夹具，#1096） */
  merchantsManifest?: string
  /** 覆盖币种域 crate 的 `crates/currencies/Cargo.toml`（依赖方向负向夹具，#1095） */
  currenciesManifest?: string
  /** 覆盖保单域 crate 的 `crates/policy/Cargo.toml`（依赖方向负向夹具，#1100） */
  policyManifest?: string
  /** 覆盖定时计划域 crate 的 `crates/scheduled/Cargo.toml`（依赖方向负向夹具，#1098） */
  scheduledManifest?: string
  /** 覆盖预算域 crate 的 `crates/budget/Cargo.toml`（依赖方向负向夹具，#1101） */
  budgetManifest?: string
  /** 覆盖实物资产域 crate 的 `crates/physical-asset/Cargo.toml`（依赖方向负向夹具，#1102） */
  physicalAssetManifest?: string
  /** 覆盖报表域 crate 的 `crates/reports/Cargo.toml`（依赖方向负向夹具，#1103） */
  reportsManifest?: string
  /** 覆盖物品域 crate 的 `crates/item/Cargo.toml`（依赖方向负向夹具，#1099） */
  itemManifest?: string
  /** 覆盖投资域 crate 的 `crates/investment/Cargo.toml`（依赖方向负向夹具，#1097） */
  investmentManifest?: string
  /** 覆盖仪表盘域 crate 的 `crates/dashboard/Cargo.toml`（依赖方向负向夹具，#1104） */
  dashboardManifest?: string
  /** 覆盖行情同步域 crate 的 `crates/market-sync/Cargo.toml`（依赖方向负向夹具，#1106） */
  marketSyncManifest?: string
  /** 覆盖多端同步域 crate 的 `crates/sync-engine/Cargo.toml`（依赖方向负向夹具，#1107） */
  syncEngineManifest?: string
  /** 覆盖 `crates/infra/src/lib.rs` 内容（test_utils cfg 门负向夹具） */
  infraLibRs?: string
  /** 覆盖 `crates/infra/src/error.rs` 内容（http 投影 impl cfg 门负向夹具） */
  infraErrorRs?: string
  /** 覆盖 `crates/sync-protocol/Cargo.toml` 内容（域侧 http 启用负向夹具） */
  protocolManifest?: string
  /** 覆盖 `src/api_server/handlers/import.rs` 内容（投资五节锚点 cfg 门负向夹具，#1185） */
  apiServerImportRs?: string
  /** 覆盖 `src/api_server/mod.rs` 内容（投资五节锚点再导出 cfg 门负向夹具，#1185） */
  apiServerModRs?: string
  /** 覆盖根包 `[dependencies]` 的 ledger-infra 行（生产依赖接线负向夹具） */
  rootInfraProdDep?: string
  /** 追加到根包 `[features]` 段的原文行（default feature 负向夹具） */
  rootFeaturesExtra?: string
  checkSh?: string
  testSh?: string
  lintFixSh?: string
  /** 覆盖 `scripts/test-exec.ts`（cargo 命令宿主，workspace 范围负向夹具，#1112） */
  testExecTs?: string
  workflow?: string
  /** 追加一个未登记的成员目录（crates/<name>）——新 crate 漏登记的负向夹具 */
  orphanCrate?: string
}

/** 六件套 deny 声明（workspace 级唯一声明处，ADR-0060） */
const LINT_DENIES = [
  'unwrap_used = "deny"',
  'expect_used = "deny"',
  'panic = "deny"',
  'todo = "deny"',
  'unimplemented = "deny"',
  'unreachable = "deny"',
].join('\n')

/**
 * 建 workspace 骨架夹具：`<root>/src-tauri/{Cargo.toml,src,crates/infra}` +
 * 仓库根的门槛宿主（scripts/check.sh、scripts/test.sh、.github/workflows/build.yml）。
 * 返回脚本参数 `[src-dir, src-tauri-dir]`——src-tauri-dir 指向夹具使 crate 边界
 * 核对落在夹具上（缺省时核对真实仓库，见既有用例）。
 */
function makeCrateFixture(overrides: CrateFixtureOverrides = {}): string[] {
  const root = mkdtempSync(join(tmpdir(), 'check-structure-crate-'))
  tempDirs.push(root)
  const srcTauri = join(root, 'src-tauri')
  populateWhitelistEntries(srcTauri)

  const rootManifest =
    overrides.rootManifest ??
    [
      '[package]',
      'name = "tauri-app"',
      'version = "0.6.0"',
      'edition = "2024"',
      '',
      '[features]',
      'test-utils = ["ledger-infra/test-utils"]',
      overrides.rootFeaturesExtra ?? '',
      '',
      '[dependencies]',
      overrides.rootInfraProdDep ?? 'ledger-infra = { path = "crates/infra" }',
      '',
      '[workspace]',
      'members = ["crates/*"]',
      'resolver = "3"',
      '',
      '[workspace.lints.clippy]',
      LINT_DENIES,
      '',
      '[dev-dependencies]',
      'tauri-app = { path = ".", features = ["test-utils"] }',
      '',
      '[lints]',
      'workspace = true',
      '',
    ].join('\n')
  writeFileSync(join(srcTauri, 'Cargo.toml'), rootManifest)

  // 成员清单默认与真实仓库同形：http 投影门（#1133）——axum optional +
  // `http = ["dep:axum"]`，default 不含 http。
  const memberManifest =
    overrides.memberManifest ??
    [
      '[package]',
      'name = "ledger-infra"',
      'version = "0.6.0"',
      'edition = "2024"',
      '',
      '[features]',
      'test-utils = []',
      'http = ["dep:axum"]',
      '',
      '[dependencies]',
      'axum = { version = "0.8", optional = true }',
      '',
      '[lints]',
      'workspace = true',
      '',
    ].join('\n')
  mkdirSync(join(srcTauri, 'crates', 'infra', 'src'), { recursive: true })
  writeFileSync(join(srcTauri, 'crates', 'infra', 'Cargo.toml'), memberManifest)
  writeFileSync(
    join(srcTauri, 'crates', 'infra', 'src', 'lib.rs'),
    overrides.infraLibRs ??
      'pub fn stub() {}\n' +
        '#[cfg(any(test, feature = "test-utils"))]\n' +
        '#[doc(hidden)]\n' +
        'pub mod test_utils;\n',
  )
  writeFileSync(
    join(srcTauri, 'crates', 'infra', 'src', 'error.rs'),
    overrides.infraErrorRs ??
      'pub struct AppError;\n' +
        '#[cfg(feature = "http")]\n' +
        'impl axum::response::IntoResponse for AppError {\n' +
        '    fn into_response(self) -> axum::response::Response {}\n' +
        '}\n',
  )
  mkdirSync(join(srcTauri, 'src'), { recursive: true })
  // 根包 lib.rs 与真实仓库同形（#1108）：test_utils 再导出面已清除——测试器具
  // 经 dev-dependency 以 ledger_infra::test_utils 直达，根包侧不再有门条目。
  writeFileSync(join(srcTauri, 'src', 'lib.rs'), 'pub fn stub() {}\n')

  // 投资五节标题锚点（#1185）：夹具与真实仓库同形——常量住 handlers/import.rs、
  // 经 api_server/mod.rs 再导出，均带「放行测试」cfg 门（生产编译门默认绿）。
  mkdirSync(join(srcTauri, 'src', 'api_server', 'handlers'), { recursive: true })
  writeFileSync(
    join(srcTauri, 'src', 'api_server', 'handlers', 'import.rs'),
    overrides.apiServerImportRs ??
      '#[cfg(any(test, feature = "test-utils"))]\n' +
        '#[doc(hidden)]\n' +
        'pub const INVESTMENT_SECTION_HEADERS: [&str; 5] = ["## 节"];\n',
  )
  writeFileSync(
    join(srcTauri, 'src', 'api_server', 'mod.rs'),
    overrides.apiServerModRs ??
      '#[cfg(any(test, feature = "test-utils"))]\n' +
        '#[doc(hidden)]\n' +
        'pub use handlers::import::INVESTMENT_SECTION_HEADERS;\n',
  )

  // 备份域 crate（#1091，首个业务域 crate）：夹具与真实仓库同形——成员目录 +
  // 门禁继承 + dev-dependency 测试环（tauri-app 供测试工厂复用，spec #1086）。
  mkdirSync(join(srcTauri, 'crates', 'backup', 'src'), { recursive: true })
  writeFileSync(
    join(srcTauri, 'crates', 'backup', 'Cargo.toml'),
    overrides.backupManifest ??
      [
        '[package]',
        'name = "ledger-backup"',
        'version = "0.6.0"',
        'edition = "2024"',
        '',
        '[dev-dependencies]',
        'tauri-app = { path = "../.." }',
        '',
        '[lints]',
        'workspace = true',
        '',
      ].join('\n'),
  )
  writeFileSync(join(srcTauri, 'crates', 'backup', 'src', 'lib.rs'), 'pub fn stub() {}\n')

  // 核心交易域 crate（#1092，P2 首个底层业务域 crate）：夹具与真实仓库同形——
  // 成员目录 + 门禁继承 + dev-dependency 测试环。
  mkdirSync(join(srcTauri, 'crates', 'transaction', 'src'), { recursive: true })
  writeFileSync(
    join(srcTauri, 'crates', 'transaction', 'Cargo.toml'),
    overrides.transactionManifest ??
      [
        '[package]',
        'name = "ledger-transaction"',
        'version = "0.6.0"',
        'edition = "2024"',
        '',
        '[dev-dependencies]',
        'tauri-app = { path = "../.." }',
        '',
        '[lints]',
        'workspace = true',
        '',
      ].join('\n'),
  )
  writeFileSync(join(srcTauri, 'crates', 'transaction', 'src', 'lib.rs'), 'pub fn stub() {}\n')

  // 账户域 crate（#1093，P3 叶子业务域 crate）：夹具与真实仓库同形——成员目录 +
  // 门禁继承 + dev-dependency 测试环。
  mkdirSync(join(srcTauri, 'crates', 'accounts', 'src'), { recursive: true })
  writeFileSync(
    join(srcTauri, 'crates', 'accounts', 'Cargo.toml'),
    overrides.accountsManifest ??
      [
        '[package]',
        'name = "ledger-accounts"',
        'version = "0.6.0"',
        'edition = "2024"',
        '',
        '[dev-dependencies]',
        'tauri-app = { path = "../.." }',
        '',
        '[lints]',
        'workspace = true',
        '',
      ].join('\n'),
  )
  writeFileSync(join(srcTauri, 'crates', 'accounts', 'src', 'lib.rs'), 'pub fn stub() {}\n')

  // 分类域 crate（#1094，P3 叶子域）：夹具与真实仓库同形——成员目录 + 门禁继承
  // + dev-dependency 测试环。
  mkdirSync(join(srcTauri, 'crates', 'categories', 'src'), { recursive: true })
  writeFileSync(
    join(srcTauri, 'crates', 'categories', 'Cargo.toml'),
    overrides.categoriesManifest ??
      [
        '[package]',
        'name = "ledger-categories"',
        'version = "0.6.0"',
        'edition = "2024"',
        '',
        '[dev-dependencies]',
        'tauri-app = { path = "../.." }',
        '',
        '[lints]',
        'workspace = true',
        '',
      ].join('\n'),
  )
  writeFileSync(join(srcTauri, 'crates', 'categories', 'src', 'lib.rs'), 'pub fn stub() {}\n')

  // 商户域 crate（#1096，参考数据域独立 crate）：夹具与真实仓库同形——成员目录 +
  // 门禁继承 + dev-dependency 测试环。
  mkdirSync(join(srcTauri, 'crates', 'merchants', 'src'), { recursive: true })
  writeFileSync(
    join(srcTauri, 'crates', 'merchants', 'Cargo.toml'),
    overrides.merchantsManifest ??
      [
        '[package]',
        'name = "ledger-merchants"',
        'version = "0.6.0"',
        'edition = "2024"',
        '',
        '[dev-dependencies]',
        'tauri-app = { path = "../.." }',
        '',
        '[lints]',
        'workspace = true',
        '',
      ].join('\n'),
  )
  writeFileSync(join(srcTauri, 'crates', 'merchants', 'src', 'lib.rs'), 'pub fn stub() {}\n')

  // 币种域 crate（#1095，P3 叶子域）：夹具与真实仓库同形——成员目录 + 门禁继承 +
  // dev-dependency 测试环。
  mkdirSync(join(srcTauri, 'crates', 'currencies', 'src'), { recursive: true })
  writeFileSync(
    join(srcTauri, 'crates', 'currencies', 'Cargo.toml'),
    overrides.currenciesManifest ??
      [
        '[package]',
        'name = "ledger-currencies"',
        'version = "0.6.0"',
        'edition = "2024"',
        '',
        '[dev-dependencies]',
        'tauri-app = { path = "../.." }',
        '',
        '[lints]',
        'workspace = true',
        '',
      ].join('\n'),
  )
  writeFileSync(join(srcTauri, 'crates', 'currencies', 'src', 'lib.rs'), 'pub fn stub() {}\n')

  // 保单域 crate（#1100，P3 叶子业务域 crate）：夹具与真实仓库同形——成员目录 +
  // 门禁继承 + dev-dependency 测试环。
  mkdirSync(join(srcTauri, 'crates', 'policy', 'src'), { recursive: true })
  writeFileSync(
    join(srcTauri, 'crates', 'policy', 'Cargo.toml'),
    overrides.policyManifest ??
      [
        '[package]',
        'name = "ledger-policy"',
        'version = "0.6.0"',
        'edition = "2024"',
        '',
        '[dev-dependencies]',
        'tauri-app = { path = "../.." }',
        '',
        '[lints]',
        'workspace = true',
        '',
      ].join('\n'),
  )
  writeFileSync(join(srcTauri, 'crates', 'policy', 'src', 'lib.rs'), 'pub fn stub() {}\n')

  // 定时计划域 crate（#1098，P3 业务域）：夹具与真实仓库同形——成员目录 + 门禁继承
  // + dev-dependency 测试环（真实 crate 生产依赖面另有基础设施/协议/核心交易三行，
  // 与依赖方向核对无关，夹具从简同其他域成员）。
  mkdirSync(join(srcTauri, 'crates', 'scheduled', 'src'), { recursive: true })
  writeFileSync(
    join(srcTauri, 'crates', 'scheduled', 'Cargo.toml'),
    overrides.scheduledManifest ??
      [
        '[package]',
        'name = "ledger-scheduled"',
        'version = "0.6.0"',
        'edition = "2024"',
        '',
        '[dev-dependencies]',
        'tauri-app = { path = "../.." }',
        '',
        '[lints]',
        'workspace = true',
        '',
      ].join('\n'),
  )
  writeFileSync(join(srcTauri, 'crates', 'scheduled', 'src', 'lib.rs'), 'pub fn stub() {}\n')

  // 预算域 crate（#1101，P3 叶子业务域 crate）：夹具与真实仓库同形——成员目录 +
  // 门禁继承 + dev-dependency 测试环。
  mkdirSync(join(srcTauri, 'crates', 'budget', 'src'), { recursive: true })
  writeFileSync(
    join(srcTauri, 'crates', 'budget', 'Cargo.toml'),
    overrides.budgetManifest ??
      [
        '[package]',
        'name = "ledger-budget"',
        'version = "0.6.0"',
        'edition = "2024"',
        '',
        '[dev-dependencies]',
        'tauri-app = { path = "../.." }',
        '',
        '[lints]',
        'workspace = true',
        '',
      ].join('\n'),
  )
  writeFileSync(join(srcTauri, 'crates', 'budget', 'src', 'lib.rs'), 'pub fn stub() {}\n')

  // 实物资产域 crate（#1102，P3 叶子业务域 crate）：夹具与真实仓库同形——成员目录 +
  // 门禁继承 + dev-dependency 测试环。
  mkdirSync(join(srcTauri, 'crates', 'physical-asset', 'src'), { recursive: true })
  writeFileSync(
    join(srcTauri, 'crates', 'physical-asset', 'Cargo.toml'),
    overrides.physicalAssetManifest ??
      [
        '[package]',
        'name = "ledger-physical-asset"',
        'version = "0.6.0"',
        'edition = "2024"',
        '',
        '[dev-dependencies]',
        'tauri-app = { path = "../.." }',
        '',
        '[lints]',
        'workspace = true',
        '',
      ].join('\n'),
  )
  writeFileSync(join(srcTauri, 'crates', 'physical-asset', 'src', 'lib.rs'), 'pub fn stub() {}\n')

  // 报表域 crate（#1103，P3 叶子业务域 crate）：夹具与真实仓库同形——成员目录 +
  // 门禁继承 + dev-dependency 测试环。
  mkdirSync(join(srcTauri, 'crates', 'reports', 'src'), { recursive: true })
  writeFileSync(
    join(srcTauri, 'crates', 'reports', 'Cargo.toml'),
    overrides.reportsManifest ??
      [
        '[package]',
        'name = "ledger-reports"',
        'version = "0.6.0"',
        'edition = "2024"',
        '',
        '[dev-dependencies]',
        'tauri-app = { path = "../.." }',
        '',
        '[lints]',
        'workspace = true',
        '',
      ].join('\n'),
  )
  writeFileSync(join(srcTauri, 'crates', 'reports', 'src', 'lib.rs'), 'pub fn stub() {}\n')

  // 物品域 crate（#1099，P3 叶子域）：夹具与真实仓库同形——成员目录 + 门禁继承 +
  // dev-dependency 测试环。
  mkdirSync(join(srcTauri, 'crates', 'item', 'src'), { recursive: true })
  writeFileSync(
    join(srcTauri, 'crates', 'item', 'Cargo.toml'),
    overrides.itemManifest ??
      [
        '[package]',
        'name = "ledger-item"',
        'version = "0.6.0"',
        'edition = "2024"',
        '',
        '[dev-dependencies]',
        'tauri-app = { path = "../.." }',
        '',
        '[lints]',
        'workspace = true',
        '',
      ].join('\n'),
  )
  writeFileSync(join(srcTauri, 'crates', 'item', 'src', 'lib.rs'), 'pub fn stub() {}\n')

  // 投资域 crate（#1097，P3 业务域）：夹具与真实仓库同形——成员目录 + 门禁继承 +
  // dev-dependency 测试环（真实 crate 生产依赖面另有基础设施/协议/核心交易/账户/
  // 币种五行，与依赖方向核对无关，夹具从简同其他域成员）。
  mkdirSync(join(srcTauri, 'crates', 'investment', 'src'), { recursive: true })
  writeFileSync(
    join(srcTauri, 'crates', 'investment', 'Cargo.toml'),
    overrides.investmentManifest ??
      [
        '[package]',
        'name = "ledger-investment"',
        'version = "0.6.0"',
        'edition = "2024"',
        '',
        '[dev-dependencies]',
        'tauri-app = { path = "../.." }',
        '',
        '[lints]',
        'workspace = true',
        '',
      ].join('\n'),
  )
  writeFileSync(join(srcTauri, 'crates', 'investment', 'src', 'lib.rs'), 'pub fn stub() {}\n')

  // 仪表盘域 crate（#1104，P3 叶子业务域 crate）：夹具与真实仓库同形——成员目录 +
  // 门禁继承（真实 crate 无 dev-dependency 环：域内无测试目标，三层测试全在根
  // 包侧，与 sync-protocol 夹具同款不带 dev-dep）。
  mkdirSync(join(srcTauri, 'crates', 'dashboard', 'src'), { recursive: true })
  writeFileSync(
    join(srcTauri, 'crates', 'dashboard', 'Cargo.toml'),
    overrides.dashboardManifest ??
      [
        '[package]',
        'name = "ledger-dashboard"',
        'version = "0.6.0"',
        'edition = "2024"',
        '',
        '[lints]',
        'workspace = true',
        '',
      ].join('\n'),
  )
  writeFileSync(join(srcTauri, 'crates', 'dashboard', 'src', 'lib.rs'), 'pub fn stub() {}\n')

  // 行情同步域 crate（#1106，P4 首个业务域 crate）：夹具与真实仓库同形——成员目录 +
  // 门禁继承 + dev-dependency 测试环（真实 crate 生产依赖面另有基础设施/同步协议/
  // 核心交易/投资四行与数据面惯用库，与依赖方向核对无关，夹具从简同其他域成员）。
  mkdirSync(join(srcTauri, 'crates', 'market-sync', 'src'), { recursive: true })
  writeFileSync(
    join(srcTauri, 'crates', 'market-sync', 'Cargo.toml'),
    overrides.marketSyncManifest ??
      [
        '[package]',
        'name = "ledger-market-sync"',
        'version = "0.6.0"',
        'edition = "2024"',
        '',
        '[dev-dependencies]',
        'tauri-app = { path = "../.." }',
        '',
        '[lints]',
        'workspace = true',
        '',
      ].join('\n'),
  )
  writeFileSync(join(srcTauri, 'crates', 'market-sync', 'src', 'lib.rs'), 'pub fn stub() {}\n')

  // 多端同步域 crate（#1107，P4 业务域 crate）：夹具与真实仓库同形——成员目录 +
  // 门禁继承 + dev-dependency 测试环（生产依赖面另有一组基础设施/协议/域依赖，
  // 与依赖方向核对无关，夹具从简同其他域成员）。
  mkdirSync(join(srcTauri, 'crates', 'sync-engine', 'src'), { recursive: true })
  writeFileSync(
    join(srcTauri, 'crates', 'sync-engine', 'Cargo.toml'),
    overrides.syncEngineManifest ??
      [
        '[package]',
        'name = "ledger-sync-engine"',
        'version = "0.6.0"',
        'edition = "2024"',
        '',
        '[dev-dependencies]',
        'tauri-app = { path = "../.." }',
        '',
        '[lints]',
        'workspace = true',
        '',
      ].join('\n'),
  )
  writeFileSync(join(srcTauri, 'crates', 'sync-engine', 'src', 'lib.rs'), 'pub fn stub() {}\n')

  // 同步协议 crate（#1089）：夹具与真实仓库同形——成员目录 + 门禁继承。
  mkdirSync(join(srcTauri, 'crates', 'sync-protocol', 'src'), { recursive: true })
  writeFileSync(
    join(srcTauri, 'crates', 'sync-protocol', 'Cargo.toml'),
    overrides.protocolManifest ??
      [
        '[package]',
        'name = "ledger-sync-protocol"',
        'version = "0.6.0"',
        'edition = "2024"',
        '',
        '[lints]',
        'workspace = true',
        '',
      ].join('\n'),
  )
  writeFileSync(join(srcTauri, 'crates', 'sync-protocol', 'src', 'lib.rs'), 'pub fn stub() {}\n')

  if (overrides.orphanCrate) {
    mkdirSync(join(srcTauri, 'crates', overrides.orphanCrate, 'src'), { recursive: true })
    writeFileSync(
      join(srcTauri, 'crates', overrides.orphanCrate, 'Cargo.toml'),
      '[package]\nname = "orphan"\nversion = "0.1.0"\nedition = "2024"\n',
    )
    writeFileSync(
      join(srcTauri, 'crates', overrides.orphanCrate, 'src', 'lib.rs'),
      'pub fn stub() {}\n',
    )
  }

  mkdirSync(join(root, 'scripts'), { recursive: true })
  mkdirSync(join(root, '.github', 'workflows'), { recursive: true })
  writeFileSync(
    join(root, 'scripts', 'check.sh'),
    overrides.checkSh ??
      '( cd src-tauri && cargo clippy --workspace --all-targets --all-features -- -D warnings )\n' +
        '( cd src-tauri && cargo fmt --all -- --check )\n',
  )
  writeFileSync(
    join(root, 'scripts', 'test.sh'),
    overrides.testSh ?? '( cd src-tauri && cargo test --workspace )\n',
  )
  writeFileSync(
    join(root, 'scripts', 'lint-fix.sh'),
    overrides.lintFixSh ??
      '( cd src-tauri && cargo fmt --all && cargo clippy --fix --workspace --all-targets --all-features --allow-dirty --allow-staged )\n',
  )
  writeFileSync(
    join(root, 'scripts', 'test-exec.ts'),
    overrides.testExecTs ??
      '// 构建一次（口径说明：`cargo test` 默认执行面）\n' +
        'const BUILD = "cargo test --workspace --no-run"\n' +
        'runChild(cargo, ["test", "--workspace", "--no-run"], { cwd: BUILD })\n',
  )
  writeFileSync(
    join(root, '.github', 'workflows', 'build.yml'),
    overrides.workflow ??
      [
        'jobs:',
        '  b:',
        '    steps:',
        '      - run: cargo test --workspace --lib --test "*"',
        '      - run: cargo fmt --all --check',
        '      - run: cargo clippy --workspace --all-targets --all-features -- -D warnings',
        '',
      ].join('\n'),
  )

  return [join(srcTauri, 'src'), srcTauri]
}

describe('check-structure crate 边界核对（spec #1086 / issue #1087 门禁前置）', () => {
  it('workspace 骨架夹具：成员登记 + 门禁继承 + 命令覆盖齐全 → 通过', () => {
    const r = run(makeCrateFixture())
    expect(r.status).toBe(0)
    expect(r.output).toContain(`crate 边界 ${CRATES.length} 个`)
  })

  it('真实仓库默认通过：crate 边界核对入摘要', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain(`crate 边界 ${CRATES.length} 个`)
  })

  it('成员 crate 删掉 [lints] workspace = true → 红（门禁继承删除即变红）', () => {
    const args = makeCrateFixture({
      memberManifest: '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('门禁继承')
    expect(r.output).toContain('ledger-infra')
  })

  it('根包删掉 [lints] workspace = true → 红', () => {
    const args = makeCrateFixture({
      rootManifest: [
        '[package]',
        'name = "tauri-app"',
        'version = "0.6.0"',
        'edition = "2024"',
        '',
        '[workspace]',
        'members = ["crates/*"]',
        'resolver = "3"',
        '',
        '[workspace.lints.clippy]',
        LINT_DENIES,
        '',
      ].join('\n'),
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('根包缺 [lints] workspace = true')
  })

  it('新增成员目录未登记 CRATES → 红（成员登记删除即变红）', () => {
    const args = makeCrateFixture({ orphanCrate: 'newdomain' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('未登记 CRATES')
    expect(r.output).toContain('crates/newdomain')
  })

  it('workspace members 未用 crates/* glob → 红', () => {
    const args = makeCrateFixture({
      rootManifest: [
        '[package]',
        'name = "tauri-app"',
        'version = "0.6.0"',
        'edition = "2024"',
        '',
        '[workspace]',
        'members = ["crates/infra"]',
        'resolver = "3"',
        '',
        '[workspace.lints.clippy]',
        LINT_DENIES,
        '',
        '[lints]',
        'workspace = true',
        '',
      ].join('\n'),
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('crates/*')
  })

  it('静态检查/测试命令缺 --workspace → 红（命令覆盖全成员删除即变红）', () => {
    const args = makeCrateFixture({ testSh: '( cd src-tauri && cargo test )\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('缺 --workspace')
    expect(r.output).toContain('test.sh')
  })

  it('test-exec.ts（新 cargo 命令宿主，.ts 形态）缺 --workspace → 红（#1112 登记）', () => {
    const args = makeCrateFixture({ testExecTs: 'const BUILD = "cargo test --no-run"\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('缺 --workspace')
    expect(r.output).toContain('test-exec.ts')
  })

  it('test-exec.ts 注释里的 `cargo test` 不算命令（.ts 注释掩码，不假红）', () => {
    const args = makeCrateFixture({
      testExecTs:
        '// 口径说明：`cargo test` 默认执行面\n' +
        '/** 另一处：cargo test --no-run */\n' +
        'const BUILD = "cargo test --workspace --no-run"\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('test-exec.ts 数组形态命令（真实命令面）缺 --workspace → 红（#1112 P2 登记生效）', () => {
    // 真实 cargo 调用是程序化数组 `runChild(cargo, ['test', …])`，逐行字面量扫描只
    // 看得见 console.log 的说明文字——登记若只匹配字面量就流于装饰（#1112 第三轮审查
    // P2）。数组形态核对让登记真正约束命令面。
    const args = makeCrateFixture({ testExecTs: "runChild(cargo, ['test', '--no-run'], { cwd: root })\n" })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('数组形态缺')
    expect(r.output).toContain('test-exec.ts')
  })

  it('test-exec.ts 数组形态命令带 --workspace → 通过（数组面单独成立，不靠字面量兜底）', () => {
    const args = makeCrateFixture({
      testExecTs: "runChild(cargo, ['test', '--workspace', '--no-run'], { cwd: root })\n",
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('命令被 echo 包成说明文字 → 红（引号内不是命令面，拒绝空集假绿，#1112 P1）', () => {
    // 相对固定点的假绿：三条真命令全包成 `echo "…cargo test…"` 后，逐行字面量扫描
    // 仍把引号内的字样当成命令，核对全绿而实际一条测试都没跑。
    const args = makeCrateFixture({ testSh: 'echo "( cd src-tauri && cargo test --workspace )"\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('未发现任何命令位置上的 cargo 命令')
    expect(r.output).toContain('空集假绿')
  })

  it('`--all-targets` 等 `--all*` 旗标不算 workspace 范围 → 红（防假绿回归）', () => {
    // 改版前的 build.yml clippy 形态：只有 --all-targets / --all-features，没有
    // --workspace。用 \b 匹配 --all 会误判为已覆盖，本用例锁死该假绿。
    const args = makeCrateFixture({
      lintFixSh:
        '( cd src-tauri && cargo clippy --fix --all-targets --all-features --allow-dirty --allow-staged )\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('缺 --workspace')
    expect(r.output).toContain('lint-fix.sh')
  })

  it('cargo fmt 缺 --all → 红（fmt 的 workspace 别名是 --all）', () => {
    const args = makeCrateFixture({ checkSh: '( cd src-tauri && cargo fmt -- --check )\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('缺 --workspace')
    expect(r.output).toContain('check.sh')
  })

  it('基础设施 crate 反向依赖壳层 crate → 红（依赖方向核对）', () => {
    const args = makeCrateFixture({
      memberManifest:
        '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('crate 依赖方向')
    expect(r.output).toContain('ledger-infra')
  })

  it('基础设施 crate 测试以 dev-dependency 反向依赖壳层 → 绿（测试专用边，spec #1086）', () => {
    const args = makeCrateFixture({
      memberManifest:
        '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[features]\nhttp = ["dep:axum"]\n\n' +
        '[dependencies]\naxum = { version = "0.8", optional = true }\n\n' +
        '[dev-dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('币种域 crate 生产依赖壳层 crate → 红（依赖方向核对，编译期拒绝的机器面，#1095）', () => {
    const args = makeCrateFixture({
      currenciesManifest:
        '[package]\nname = "ledger-currencies"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('crate 依赖方向')
    expect(r.output).toContain('ledger-currencies')
  })
})

describe('check-structure 报表域 crate（#1103 P3 叶子业务域 crate 自根包拆出）', () => {
  it('真实仓库默认通过：报表域 crate 模块级扫描入摘要', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain(`报表域模块 ${REPORTS_MODULES.length} 项`)
  })

  it('报表域 crate 模块引用壳层 → 红并定位文件行号', () => {
    // 报表域唯一模块 model.rs 与交易域清单撞名（路由优先级归交易 crate，与
    // placeOverride 先后链一致），夹具直接写入报表域 crate 的模块路径。
    const args = makeFixture()
    writeFileSync(join(args[1], REPORTS_SRC_REL, 'model.rs'), shellUse)
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('反向依赖')
    expect(r.output).toContain('model.rs:1')
  })

  it('报表域 crate 模块引用同步域 → 红（业务域→同步域零容忍覆盖 crate）', () => {
    const args = makeFixture()
    writeFileSync(
      join(args[1], REPORTS_SRC_REL, 'model.rs'),
      'use tauri_app_lib::sync_engine::engine::ReplayEffect;\n',
    )
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('model.rs:1')
  })

  it('报表域 crate 生产依赖壳层 crate → 红（依赖方向核对，编译期拒绝的机器面，#1103）', () => {
    const args = makeCrateFixture({
      reportsManifest:
        '[package]\nname = "ledger-reports"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('crate 依赖方向')
    expect(r.output).toContain('ledger-reports')
  })

  it('报表域 crate 生产依赖核心交易域 crate → 绿（域→域合法上层依赖，汇总口径消费度量矩阵，#1092）', () => {
    // 汇总口径由核心交易域 kind→度量矩阵单一真源驱动（reports → transaction
    // 单向），依赖面受 AC 约束（基础设施/协议/核心交易域），不属禁边。
    const args = makeCrateFixture({
      reportsManifest:
        '[package]\nname = "ledger-reports"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\nledger-transaction = { path = "../transaction" }\n\n' +
        '[dev-dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('报表域 crate 测试以 dev-dependency 反向依赖壳层 → 绿（测试专用边，spec #1086）', () => {
    // 缺省 reportsManifest 即该形态（与真实 crate 同形），单列用例锁死语义。
    const r = run(makeCrateFixture())
    expect(r.status).toBe(0)
  })
})

describe('check-structure 仪表盘域 crate（#1104 P3 叶子业务域 crate 自根包拆出）', () => {
  it('真实仓库默认通过：仪表盘域 crate 模块级扫描入摘要', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain(`仪表盘域模块 ${DASHBOARD_MODULES.length} 项`)
  })

  it('仪表盘域 crate 模块引用壳层 → 红并定位文件行号', () => {
    // 仪表盘域模块 model.rs 与交易域清单撞名（路由优先级归交易 crate，与
    // placeOverride 先后链一致），夹具直接写入仪表盘域 crate 的模块路径。
    const args = makeFixture()
    writeFileSync(join(args[1], DASHBOARD_SRC_REL, 'model.rs'), shellUse)
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('反向依赖')
    expect(r.output).toContain('model.rs:1')
  })

  it('仪表盘域 crate 模块引用同步域 → 红（业务域→同步域零容忍覆盖 crate）', () => {
    const args = makeFixture()
    writeFileSync(
      join(args[1], DASHBOARD_SRC_REL, 'model.rs'),
      'use tauri_app_lib::sync_engine::engine::ReplayEffect;\n',
    )
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('model.rs:1')
  })

  it('仪表盘域 crate 生产依赖壳层 crate → 红（依赖方向核对，编译期拒绝的机器面，#1104）', () => {
    const args = makeCrateFixture({
      dashboardManifest:
        '[package]\nname = "ledger-dashboard"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('crate 依赖方向')
    expect(r.output).toContain('ledger-dashboard')
  })

  it('仪表盘域 crate 生产依赖账户域与实物资产域 crate → 绿（域→域合法上层依赖，余额口径与第三腿，#1104 修订 AC）', () => {
    // 余额腿经账户域单一读口径、实物资产第三腿经实物资产域在持合计单一读
    // 口径（dashboard → accounts/physical-asset 单向），依赖面受修订后 AC
    // 约束（基础设施/协议/核心交易域/账户域/实物资产域），不属禁边。
    const args = makeCrateFixture({
      dashboardManifest:
        '[package]\nname = "ledger-dashboard"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\n' +
        'ledger-accounts = { path = "../accounts" }\n' +
        'ledger-physical-asset = { path = "../physical-asset" }\n\n' +
        '[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('仪表盘域 crate 测试以 dev-dependency 反向依赖壳层 → 绿（测试专用边，spec #1086）', () => {
    // 真实 crate 域内无测试目标、无 dev-dep 环；本用例锁「将来域内测试立项时
    // 按 ledger-reports 先例引入 dev-dependency 环仍合法」的语义。
    const args = makeCrateFixture({
      dashboardManifest:
        '[package]\nname = "ledger-dashboard"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dev-dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })
})

describe('check-structure 保单域 crate（#1100 P3 叶子业务域 crate 自根包拆出）', () => {
  it('真实仓库默认通过：保单域 crate 模块级扫描入摘要', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain(`保单域模块 ${POLICY_MODULES.length} 项`)
  })

  it('保单域 crate 模块引用壳层 → 红并定位文件行号', () => {
    // 'stats.rs' 无撞名（command.rs / model.rs 与交易域清单、crud.rs 与商户域
    // 清单撞名，路由优先级归先登记的 crate），经 placeOverride 落保单域 crate。
    const args = makeFixture({ 'stats.rs': shellUse })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('反向依赖')
    expect(r.output).toContain('stats.rs:1')
  })

  it('保单域 crate 模块引用同步域 → 红（业务域→同步域零容忍覆盖 crate）', () => {
    const args = makeFixture({
      'stats.rs': 'use tauri_app_lib::sync_engine::engine::ReplayEffect;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('stats.rs:1')
  })

  it('保单域 crate 生产依赖壳层 crate → 红（依赖方向核对，编译期拒绝的机器面，#1100）', () => {
    const args = makeCrateFixture({
      policyManifest:
        '[package]\nname = "ledger-policy"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('crate 依赖方向')
    expect(r.output).toContain('ledger-policy')
  })

  it('保单域 crate 生产依赖核心交易域 crate → 绿（域→域合法上层依赖，接缝实现注册侧，#1092）', () => {
    // 交易×保单接缝（来源列①反查）的实现注册是保单 → 核心交易的单向依赖（#1092
    // 挂载点⑤反转后的合法方向），依赖面受 AC 约束（基础设施/协议/核心交易域），
    // 不属禁边。
    const args = makeCrateFixture({
      policyManifest:
        '[package]\nname = "ledger-policy"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\nledger-transaction = { path = "../transaction" }\n\n' +
        '[dev-dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('保单域 crate 测试以 dev-dependency 反向依赖壳层 → 绿（测试专用边，spec #1086）', () => {
    // 缺省 policyManifest 即该形态（与真实 crate 同形），单列用例锁死语义。
    const r = run(makeCrateFixture())
    expect(r.status).toBe(0)
  })
})

describe('check-structure 预算域 crate（#1101 P3 叶子业务域 crate 自根包拆出）', () => {
  it('真实仓库默认通过：预算域 crate 模块级扫描入摘要', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain(`预算域模块 ${BUDGET_MODULES.length} 项`)
  })

  it('预算域 crate 模块引用壳层 → 红并定位文件行号', () => {
    // 'progress.rs' 无撞名（command.rs / model.rs 与交易域清单、crud.rs 与商户域
    // 清单撞名，路由优先级归先登记的 crate），经 placeOverride 落预算域 crate。
    const args = makeFixture({ 'progress.rs': shellUse })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('反向依赖')
    expect(r.output).toContain('progress.rs:1')
  })

  it('预算域 crate 模块引用同步域 → 红（业务域→同步域零容忍覆盖 crate）', () => {
    const args = makeFixture({
      'progress.rs': 'use tauri_app_lib::sync_engine::engine::ReplayEffect;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('progress.rs:1')
  })

  it('预算域 crate 生产依赖壳层 crate → 红（依赖方向核对，编译期拒绝的机器面，#1101）', () => {
    const args = makeCrateFixture({
      budgetManifest:
        '[package]\nname = "ledger-budget"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('crate 依赖方向')
    expect(r.output).toContain('ledger-budget')
  })

  it('预算域 crate 生产依赖核心交易域 crate → 绿（域→域合法上层依赖，度量矩阵消费侧，#1092）', () => {
    // 预算进度 spent 口径消费交易域 kind→度量矩阵（ExpenseNet），是预算 → 核心
    // 交易的单向依赖（域→域合法上层依赖），依赖面受 AC 约束（基础设施/协议/
    // 核心交易域），不属禁边。
    const args = makeCrateFixture({
      budgetManifest:
        '[package]\nname = "ledger-budget"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\nledger-transaction = { path = "../transaction" }\n\n' +
        '[dev-dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('预算域 crate 测试以 dev-dependency 反向依赖壳层 → 绿（测试专用边，spec #1086）', () => {
    // 缺省 budgetManifest 即该形态（与真实 crate 同形），单列用例锁死语义。
    const r = run(makeCrateFixture())
    expect(r.status).toBe(0)
  })
})

describe('check-structure 实物资产域 crate（#1102 P3 叶子业务域 crate 自根包拆出）', () => {
  it('真实仓库默认通过：实物资产域 crate 模块级扫描入摘要', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain(`实物资产域模块 ${PHYSICAL_ASSET_MODULES.length} 项`)
  })

  it('实物资产域 crate 模块引用壳层 → 红并定位文件行号', () => {
    // 本域模块文件名与先登记 crate 全部撞名（command.rs / model.rs → 交易域、
    // crud.rs → 商户域、validation.rs → 保单域，placeOverride 路由优先级归先
    // 登记 crate），经 makeFixture 后直写 crate 路径覆盖桩（writeModuleStubs
    // 已按 PHYSICAL_ASSET_MODULES 落位）。
    const args = makeFixture()
    writeFileSync(join(args[1], PHYSICAL_ASSET_SRC_REL, 'validation.rs'), shellUse)
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('反向依赖')
    expect(r.output).toContain('validation.rs:1')
  })

  it('实物资产域 crate 模块引用同步域 → 红（业务域→同步域零容忍覆盖 crate）', () => {
    // 撞名同上：直写 crate 路径覆盖桩。
    const args = makeFixture()
    writeFileSync(
      join(args[1], PHYSICAL_ASSET_SRC_REL, 'validation.rs'),
      'use tauri_app_lib::sync_engine::engine::ReplayEffect;\n',
    )
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('validation.rs:1')
  })

  it('实物资产域 crate 生产依赖壳层 crate → 红（依赖方向核对，编译期拒绝的机器面，#1102）', () => {
    const args = makeCrateFixture({
      physicalAssetManifest:
        '[package]\nname = "ledger-physical-asset"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('crate 依赖方向')
    expect(r.output).toContain('ledger-physical-asset')
  })

  it('实物资产域 crate 生产依赖核心交易域 crate → 绿（域→域合法上层依赖，Amount 口径消费，#1092）', () => {
    // 当前估值折本位币消费交易域 Amount 口径是实物资产 → 核心交易的单向依赖
    //（域间横向依赖，ADR-0056 决策 2 允许），依赖面受 AC 约束（基础设施/协议/
    // 核心交易域），不属禁边。
    const args = makeCrateFixture({
      physicalAssetManifest:
        '[package]\nname = "ledger-physical-asset"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\nledger-transaction = { path = "../transaction" }\n\n' +
        '[dev-dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('实物资产域 crate 测试以 dev-dependency 反向依赖壳层 → 绿（测试专用边，spec #1086）', () => {
    // 缺省 physicalAssetManifest 即该形态（与真实 crate 同形），单列用例锁死语义。
    const r = run(makeCrateFixture())
    expect(r.status).toBe(0)
  })
})

describe('check-structure 备份域 crate（#1091 首个业务域 crate 自根包拆出）', () => {
  it('真实仓库默认通过：备份域 crate 模块级扫描入摘要', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain(`备份域模块 ${BACKUP_MODULES.length} 项`)
  })

  it('备份域 crate 模块引用壳层 → 红并定位文件行号', () => {
    // 'auto.rs' 经 placeOverride 落备份域 crate（BACKUP_MODULES 派生路由，#1091）。
    const args = makeFixture({ 'auto.rs': shellUse })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('反向依赖')
    expect(r.output).toContain('auto.rs:1')
  })

  it('备份域 crate 模块引用同步域 → 红（业务域→同步域零容忍覆盖 crate）', () => {
    const args = makeFixture({
      'engine.rs': 'use tauri_app_lib::sync_engine::engine::ReplayEffect;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('engine.rs:1')
  })

  it('备份域 crate 生产依赖壳层 crate → 红（依赖方向核对，编译期拒绝的机器面）', () => {
    const args = makeCrateFixture({
      backupManifest:
        '[package]\nname = "ledger-backup"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('crate 依赖方向')
    expect(r.output).toContain('ledger-backup')
  })

  it('备份域 crate 测试以 dev-dependency 反向依赖壳层 → 绿（测试专用边，spec #1086）', () => {
    // 缺省 backupManifest 即该形态（与真实 crate 同形），单列用例锁死语义。
    const r = run(makeCrateFixture())
    expect(r.status).toBe(0)
  })

  it('核心交易域 crate 生产依赖壳层 crate → 红（依赖方向核对，编译期拒绝的机器面，#1092）', () => {
    const args = makeCrateFixture({
      transactionManifest:
        '[package]\nname = "ledger-transaction"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('crate 依赖方向')
    expect(r.output).toContain('ledger-transaction')
  })
})

describe('check-structure 账户域 crate（#1093 叶子业务域 crate 自根包拆出）', () => {
  it('真实仓库默认通过：账户域 crate 模块级扫描入摘要', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain(`账户域模块 ${ACCOUNTS_MODULES.length} 项`)
  })

  it('夹具与真实仓库同形：账户域 crate 默认通过', () => {
    const r = run(makeCrateFixture())
    expect(r.status).toBe(0)
  })

  it('账户域 crate 模块引用壳层 → 红并定位文件行号', () => {
    // 'core.rs' 经 placeOverride 落账户域 crate（ACCOUNTS_MODULES 派生路由，#1093）。
    const args = makeFixture({ 'core.rs': shellUse })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('反向依赖')
    expect(r.output).toContain('core.rs:1')
  })

  it('账户域 crate 模块引用同步域 → 红（业务域→同步域零容忍覆盖 crate）', () => {
    const args = makeFixture({
      'balance.rs': 'use tauri_app_lib::sync_engine::registry::dispatch;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('balance.rs:1')
  })

  it('账户域 crate 生产依赖壳层 crate → 红（依赖方向核对，编译期拒绝的机器面，#1093）', () => {
    const args = makeCrateFixture({
      accountsManifest:
        '[package]\nname = "ledger-accounts"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('crate 依赖方向')
    expect(r.output).toContain('ledger-accounts')
  })

  it('账户域 crate 测试以 dev-dependency 反向依赖壳层 → 绿（测试专用边，spec #1086）', () => {
    // 缺省 accountsManifest 即该形态（与真实 crate 同形），单列用例锁死语义。
    const r = run(makeCrateFixture())
    expect(r.status).toBe(0)
  })
})

describe('check-structure 分类域 crate（#1094 P3 叶子域自根包拆出）', () => {
  it('真实仓库默认通过：分类域 crate 模块级扫描入摘要', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain(`分类域模块 ${CATEGORIES_MODULES.length} 项`)
  })

  it('分类域 crate 模块引用壳层 → 红并定位文件行号', () => {
    // 'core.rs' 无撞名（`command.rs` / `model.rs` 与交易域清单撞名，路由优先
    // 落交易域），经 placeOverride 落分类域 crate（CATEGORIES_MODULES 派生路由）。
    const args = makeFixture({ 'core.rs': shellUse })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('反向依赖')
    expect(r.output).toContain('core.rs:1')
  })

  it('分类域 crate 模块引用同步域 → 红（业务域→同步域零容忍覆盖 crate）', () => {
    const args = makeFixture({
      'core.rs': 'use tauri_app_lib::sync_engine::engine::ReplayEffect;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('core.rs:1')
  })

  it('分类域 crate 生产依赖壳层 crate → 红（依赖方向核对，编译期拒绝的机器面）', () => {
    const args = makeCrateFixture({
      categoriesManifest:
        '[package]\nname = "ledger-categories"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('crate 依赖方向')
    expect(r.output).toContain('ledger-categories')
  })

  it('分类域 crate 测试以 dev-dependency 反向依赖壳层 → 绿（测试专用边，spec #1086）', () => {
    // 缺省 categoriesManifest 即该形态（与真实 crate 同形），单列用例锁死语义。
    const r = run(makeCrateFixture())
    expect(r.status).toBe(0)
  })
})

describe('check-structure 商户域 crate（#1096 参考数据域独立 crate 自根包拆出）', () => {
  it('真实仓库默认通过：商户域 crate 模块级扫描入摘要', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain(`商户域模块 ${MERCHANTS_MODULES.length} 项`)
  })

  it('商户域 crate 模块引用壳层 → 红并定位文件行号', () => {
    // 'crud.rs' 经 placeOverride 落商户域 crate（MERCHANTS_MODULES 派生路由，#1096；
    // command.rs / model.rs 与交易清单同名，路由优先级归交易 crate）。
    const args = makeFixture({ 'crud.rs': shellUse })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('反向依赖')
    expect(r.output).toContain('crud.rs:1')
  })

  it('商户域 crate 模块引用同步域 → 红（业务域→同步域零容忍覆盖 crate）', () => {
    const args = makeFixture({
      'crud.rs': 'use tauri_app_lib::sync_engine::engine::ReplayEffect;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('crud.rs:1')
  })

  it('商户域 crate 生产依赖壳层 crate → 红（依赖方向核对，编译期拒绝的机器面）', () => {
    const args = makeCrateFixture({
      merchantsManifest:
        '[package]\nname = "ledger-merchants"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('crate 依赖方向')
    expect(r.output).toContain('ledger-merchants')
  })

  it('商户域 crate 生产依赖核心交易域 crate → 绿（域→域合法上层依赖，接缝实现注册侧，#1092）', () => {
    // 交易×商户接缝的实现注册是商户 → 核心交易的单向依赖（#1092 挂载点⑤反转
    // 后的合法方向），依赖面受 AC 约束（基础设施/协议/核心交易域），不属禁边。
    const args = makeCrateFixture({
      merchantsManifest:
        '[package]\nname = "ledger-merchants"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\nledger-transaction = { path = "../transaction" }\n\n' +
        '[dev-dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('商户域 crate 测试以 dev-dependency 反向依赖壳层 → 绿（测试专用边，spec #1086）', () => {
    // 缺省 merchantsManifest 即该形态（与真实 crate 同形），单列用例锁死语义。
    const r = run(makeCrateFixture())
    expect(r.status).toBe(0)
  })
})

describe('check-structure 定时计划域 crate（#1098 业务域 crate 自根包拆出）', () => {
  it('真实仓库默认通过：定时计划域 crate 模块级扫描入摘要', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain(`定时计划域模块 ${SCHEDULED_MODULES.length} 项`)
  })

  it('夹具与真实仓库同形：定时计划域 crate 默认通过', () => {
    const r = run(makeCrateFixture())
    expect(r.status).toBe(0)
  })

  it('定时计划域 crate 模块引用壳层 → 红并定位文件行号', () => {
    // 'source.rs' 经 placeOverride 落定时计划域 crate（SCHEDULED_MODULES 派生路由，
    // #1098；无撞名可用——engine.rs 归备份域、command.rs 归账户域，夹具只用
    // auto_run / models / source / spend 四名）。
    const args = makeFixture({ 'source.rs': shellUse })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('反向依赖')
    expect(r.output).toContain('source.rs:1')
  })

  it('定时计划域 crate 模块引用同步域 → 红（业务域→同步域零容忍覆盖 crate）', () => {
    const args = makeFixture({
      'spend.rs': 'use tauri_app_lib::sync_engine::registry::dispatch;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('spend.rs:1')
  })

  it('定时计划域 crate 生产依赖壳层 crate → 红（依赖方向核对，编译期拒绝的机器面，#1098）', () => {
    const args = makeCrateFixture({
      scheduledManifest:
        '[package]\nname = "ledger-scheduled"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('crate 依赖方向')
    expect(r.output).toContain('ledger-scheduled')
  })

  it('定时计划域 crate 生产依赖备份域 crate → 绿（票面允许集内的域→域方向，#1098 AC）', () => {
    // AC 允许集「基础设施、协议、核心交易域与备份域」：定时 → 备份是域→域方向，
    // 分层同秩不违向（实际实现未声明该依赖——置脏/追补已按注册点反转，此处锁死
    // 「允许集内不误报」的判定语义，与商户域→交易域用例同型）。
    const args = makeCrateFixture({
      scheduledManifest:
        '[package]\nname = "ledger-scheduled"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\nledger-backup = { path = "../backup" }\n\n' +
        '[dev-dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('定时计划域 crate 测试以 dev-dependency 反向依赖壳层 → 绿（测试专用边，spec #1086）', () => {
    // 缺省 scheduledManifest 即该形态（与真实 crate 同形），单列用例锁死语义。
    const r = run(makeCrateFixture())
    expect(r.status).toBe(0)
  })
})

describe('check-structure 物品域 crate（#1099 P3 叶子域自根包拆出）', () => {
  it('真实仓库默认通过：物品域 crate 模块级扫描入摘要', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain(`物品域模块 ${ITEM_MODULES.length} 项`)
  })

  it('物品域 crate 模块引用壳层 → 红并定位文件行号', () => {
    // 'guard.rs' 无撞名（`command.rs` / `model.rs` 与交易域清单撞名，路由优先
    // 落交易域），经 placeOverride 落物品域 crate（ITEM_MODULES 派生路由，#1099）。
    const args = makeFixture({ 'guard.rs': shellUse })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('反向依赖')
    expect(r.output).toContain('guard.rs:1')
  })

  it('物品域 crate 模块引用同步域 → 红（业务域→同步域零容忍覆盖 crate）', () => {
    const args = makeFixture({
      'guard.rs': 'use tauri_app_lib::sync_engine::engine::ReplayEffect;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('guard.rs:1')
  })

  it('物品域 crate 生产依赖壳层 crate → 红（依赖方向核对，编译期拒绝的机器面）', () => {
    const args = makeCrateFixture({
      itemManifest:
        '[package]\nname = "ledger-item"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('crate 依赖方向')
    expect(r.output).toContain('ledger-item')
  })

  it('物品域 crate 生产依赖核心交易域 crate → 绿（域→域合法上层依赖，接缝实现注册侧，#1092）', () => {
    // 交易×物品来源列反查接缝的实现注册是物品 → 核心交易的单向依赖（#1092 挂载
    // 点⑤反转后的合法方向），依赖面受 AC 约束（基础设施/协议/核心交易域），不属禁边。
    const args = makeCrateFixture({
      itemManifest:
        '[package]\nname = "ledger-item"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\nledger-transaction = { path = "../transaction" }\n\n' +
        '[dev-dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('物品域 crate 测试以 dev-dependency 反向依赖壳层 → 绿（测试专用边，spec #1086）', () => {
    // 缺省 itemManifest 即该形态（与真实 crate 同形），单列用例锁死语义。
    const r = run(makeCrateFixture())
    expect(r.status).toBe(0)
  })
})

describe('check-structure 投资域 crate（#1097 业务域 crate 自根包拆出）', () => {
  it('真实仓库默认通过：投资域 crate 模块级扫描入摘要', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain(`投资域模块 ${INVESTMENT_MODULES.length} 项`)
  })

  it('投资域 crate 模块引用壳层 → 红并定位文件行号', () => {
    // 'trend.rs' 经 placeOverride 落投资域 crate（INVESTMENT_MODULES 派生路由，#1097；
    // command.rs / model.rs 与交易/账户/分类/商户/币种清单同名，路由优先级归先登记 crate）。
    const args = makeFixture({ 'trend.rs': shellUse })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('反向依赖')
    expect(r.output).toContain('trend.rs:1')
  })

  it('投资域 crate 模块引用同步域 → 红（业务域→同步域零容忍覆盖 crate）', () => {
    const args = makeFixture({
      'trend.rs': 'use tauri_app_lib::sync_engine::engine::ReplayEffect;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('trend.rs:1')
  })

  it('投资域 crate 生产依赖壳层 crate → 红（依赖方向核对，编译期拒绝的机器面）', () => {
    const args = makeCrateFixture({
      investmentManifest:
        '[package]\nname = "ledger-investment"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('crate 依赖方向')
    expect(r.output).toContain('ledger-investment')
  })

  it('投资域 crate 生产依赖核心交易域/账户域/币种域 → 绿（域→域合法上层依赖，AC 允许集）', () => {
    // 交易域接缝实现注册侧（#1092 挂载点⑤反转后的合法方向）与两条域→域上层
    // 依赖（投资 → 账户：AccountType/余额口径，spec 明文；投资 → 币种：汇率实体
    // 消费方与录入入口，#418/ADR-0059，#1097 裁决显性化承认、非新增耦合），
    // 依赖面受 AC 约束，不属禁边。
    const args = makeCrateFixture({
      investmentManifest:
        '[package]\nname = "ledger-investment"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\n' +
        'ledger-transaction = { path = "../transaction" }\n' +
        'ledger-accounts = { path = "../accounts" }\n' +
        'ledger-currencies = { path = "../currencies" }\n\n' +
        '[dev-dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('投资域 crate 测试以 dev-dependency 反向依赖壳层 → 绿（测试专用边，spec #1086）', () => {
    // 缺省 investmentManifest 即该形态（与真实 crate 同形），单列用例锁死语义。
    const r = run(makeCrateFixture())
    expect(r.status).toBe(0)
  })
})

describe('check-structure 行情同步域 crate（#1106 P4 业务域 crate 自根包拆出）', () => {
  it('真实仓库默认通过：行情同步域 crate 模块级扫描入摘要', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain(`行情同步域模块 ${MARKET_SYNC_MODULES.length} 项`)
  })

  it('行情同步域 crate 模块引用壳层 → 红并定位文件行号', () => {
    // 'http.rs' 经 placeOverride 落行情同步域 crate（MARKET_SYNC_MODULES 派生路由，
    // #1106；model.rs / progress.rs / fund.rs / stock.rs 与先登记 crate 清单同名，
    // 路由优先级归先登记 crate）。
    const args = makeFixture({ 'http.rs': shellUse })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('反向依赖')
    expect(r.output).toContain('http.rs:1')
  })

  it('行情同步域 crate 模块引用同步域 → 红（业务域→同步域零容忍覆盖 crate）', () => {
    const args = makeFixture({
      'incremental.rs': 'use tauri_app_lib::sync_engine::engine::ReplayEffect;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('incremental.rs:1')
  })

  it('行情同步域 crate 生产依赖壳层 crate → 红（依赖方向核对，编译期拒绝的机器面）', () => {
    const args = makeCrateFixture({
      marketSyncManifest:
        '[package]\nname = "ledger-market-sync"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('crate 依赖方向')
    expect(r.output).toContain('ledger-market-sync')
  })

  it('行情同步域 crate 生产依赖基础设施/协议/核心交易/投资域 → 绿（域→域合法上层依赖，AC 允许集全量）', () => {
    // 票面 AC 允许集全量四条：基础设施（db/error/events）、同步协议（op 落库行
    // device_id）、核心交易域（币种缺省推导）与投资域（价格写入单点 / 名称随行
    // 刷新 / 通道派生 / 统一报价载荷，ADR-0103）——均为上层域消费下层域的合法
    // 直呼（ADR-0112 决策 2），不属越界边。
    const args = makeCrateFixture({
      marketSyncManifest:
        '[package]\nname = "ledger-market-sync"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\n' +
        'ledger-infra = { path = "../infra" }\n' +
        'ledger-sync-protocol = { path = "../sync-protocol" }\n' +
        'ledger-transaction = { path = "../transaction" }\n' +
        'ledger-investment = { path = "../investment" }\n\n' +
        '[dev-dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('行情同步域 crate 测试以 dev-dependency 反向依赖壳层 → 绿（测试专用边，spec #1086）', () => {
    // 缺省 marketSyncManifest 即该形态（与真实 crate 同形），单列用例锁死语义。
    const r = run(makeCrateFixture())
    expect(r.status).toBe(0)
  })
})

describe('check-structure 多端同步域 crate（#1107 P4 业务域 crate 自根包拆出）', () => {
  it('真实仓库默认通过：多端同步域 crate 模块级扫描入摘要', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain(`多端同步域模块 ${SYNC_ENGINE_MODULES.length} 项`)
  })

  it('多端同步域 crate 模块引用壳层 → 红并定位文件行号', () => {
    // checkpoint.rs 经 placeOverride 落多端同步域 crate（SYNC_ENGINE_MODULES
    // 派生路由，#1107；engine.rs / command.rs / model.rs / channel.rs 与先登记
    // crate 清单同名，夹具只用无撞名条目）。
    const args = makeFixture({ 'checkpoint.rs': shellUse })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('反向依赖')
    expect(r.output).toContain('checkpoint.rs:1')
  })

  it('多端同步域 crate 生产依赖壳层 crate → 红（依赖方向核对，编译期拒绝的机器面）', () => {
    const args = makeCrateFixture({
      syncEngineManifest:
        '[package]\nname = "ledger-sync-engine"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('crate 依赖方向')
    expect(r.output).toContain('ledger-sync-engine')
  })

  it('多端同步域 crate 生产依赖基础设施/协议/核心交易与各业务域 → 绿（AC 允许集内合法单向依赖）', () => {
    const args = makeCrateFixture({
      syncEngineManifest:
        '[package]\nname = "ledger-sync-engine"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\n' +
        'ledger-infra = { path = "../infra" }\n' +
        'ledger-sync-protocol = { path = "../sync-protocol" }\n' +
        'ledger-transaction = { path = "../transaction" }\n' +
        'ledger-accounts = { path = "../accounts" }\n' +
        'ledger-scheduled = { path = "../scheduled" }\n' +
        'ledger-backup = { path = "../backup" }\n\n' +
        '[dev-dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('业务域 crate 生产依赖多端同步域 crate → 红（同层禁边，ADR-0101 决策 4b / #1107）', () => {
    const args = makeCrateFixture({
      accountsManifest:
        '[package]\nname = "ledger-accounts"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\nledger-sync-engine = { path = "../sync-engine" }\n\n' +
        '[dev-dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域 crate ledger-accounts 生产依赖多端同步域 ledger-sync-engine')
  })

  it('多端同步域 crate 新增未登记模块 → 红（模块清单双向全等，#1107）', () => {
    const args = makeFixture()
    writeFileSync(join(args[1], SYNC_ENGINE_SRC_REL, 'new_module.rs'), STUB)
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('SYNC_ENGINE_MODULES 未登记模块')
  })

  it('多端同步域 crate 测试以 dev-dependency 反向依赖壳层 → 绿（测试专用边，spec #1086）', () => {
    // 缺省 syncEngineManifest 即该形态（与真实 crate 同形），单列用例锁死语义。
    const r = run(makeCrateFixture())
    expect(r.status).toBe(0)
  })
})

describe('check-structure test_utils 生产编译门（ADR-0111 决策 5 / issue #1132）', () => {
  it('真实仓库默认通过：cfg 门 + 生产依赖不启用 test-utils', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain('test_utils 生产编译门')
  })

  it('workspace 骨架夹具默认通过', () => {
    const r = run(makeCrateFixture())
    expect(r.status).toBe(0)
  })

  it('infra lib.rs 摘掉 test_utils cfg 门 → 红（删除 cfg 门即变红）', () => {
    const args = makeCrateFixture({
      infraLibRs: 'pub fn stub() {}\n#[doc(hidden)]\npub mod test_utils;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('test_utils 生产编译门')
    expect(r.output).toContain('cfg 门')
  })

  it('模块声明前只有普通注释 → 仍红（注释不构成门）', () => {
    const args = makeCrateFixture({
      infraLibRs: 'pub fn stub() {}\n// 仅注释说明，不构成门\npub mod test_utils;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('test_utils 生产编译门')
  })

  it('cfg 门写在 doc(hidden) 之前 → 绿（属性顺序不敏感）', () => {
    const args = makeCrateFixture({
      infraLibRs:
        'pub fn stub() {}\n#[doc(hidden)]\n#[cfg(any(test, feature = "test-utils"))]\npub mod test_utils;\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('根包生产依赖 ledger-infra 启用 test-utils → 红（生产会编入测试器具）', () => {
    const args = makeCrateFixture({
      rootInfraProdDep: 'ledger-infra = { path = "crates/infra", features = ["test-utils"] }',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('test_utils 生产编译门')
    expect(r.output).toContain('生产依赖')
  })

  it('反向门 #[cfg(not(test))] → 红（模块只留给生产）', () => {
    const args = makeCrateFixture({
      infraLibRs: 'pub fn stub() {}\n#[cfg(not(test))]\npub mod test_utils;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('test_utils 生产编译门')
    expect(r.output).toContain('放行测试')
  })

  it('与测试无关的 cfg 门 → 红（等价于无门）', () => {
    const args = makeCrateFixture({
      infraLibRs: 'pub fn stub() {}\n#[cfg(debug_assertions)]\npub mod test_utils;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('test_utils 生产编译门')
  })

  it('infra [features] default 含 test-utils → 红（默认 feature 即生产编入）', () => {
    const args = makeCrateFixture({
      memberManifest:
        '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[features]\ntest-utils = []\ndefault = ["test-utils"]\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('default 包含 test-utils')
  })

  it('根包 [features] default 含 test-utils → 红', () => {
    const args = makeCrateFixture({ rootFeaturesExtra: 'default = ["test-utils"]' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('default 包含 test-utils')
  })

  it('default 经中间 feature 转发到 test-utils → 红（转发链同样生产启用）', () => {
    const args = makeCrateFixture({
      rootFeaturesExtra: 'default = ["devkit"]\ndevkit = ["ledger-infra/test-utils"]',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('default 包含 test-utils')
  })
})

describe('check-structure 投资五节锚点生产编译门（issue #1185）', () => {
  it('真实仓库默认通过：常量与再导出均带「放行测试」cfg 门', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain('投资五节锚点生产编译门')
  })

  it('workspace 骨架夹具默认通过', () => {
    const r = run(makeCrateFixture())
    expect(r.status).toBe(0)
  })

  it('锚点常量摘掉 cfg 门 → 红（删除 cfg 门即变红）', () => {
    const args = makeCrateFixture({
      apiServerImportRs:
        '#[doc(hidden)]\npub const INVESTMENT_SECTION_HEADERS: [&str; 5] = ["## 节"];\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('投资五节锚点生产编译门')
    expect(r.output).toContain('cfg 门')
  })

  it('api_server 再导出摘掉 cfg 门 → 红（测试锚点会静默进生产二进制）', () => {
    const args = makeCrateFixture({
      apiServerModRs: 'pub use handlers::import::INVESTMENT_SECTION_HEADERS;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('投资五节锚点生产编译门')
    expect(r.output).toContain('放行测试')
  })

  it('锚点声明被删除 → 红（两层锁共享面不可无声明消失）', () => {
    const args = makeCrateFixture({ apiServerImportRs: 'pub fn stub() {}\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('投资五节锚点生产编译门')
    expect(r.output).toContain('找不到')
  })
})

describe('check-structure http 投影 feature 门（ADR-0111 决策 5 / issue #1133）', () => {
  it('真实仓库默认通过：axum optional + http 门 + default 不含 http + 域侧不启用', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain('http 投影 feature 门')
  })

  it('workspace 骨架夹具默认通过', () => {
    const r = run(makeCrateFixture())
    expect(r.status).toBe(0)
  })

  it('infra axum 依赖摘掉 optional → 红（裸依赖即无条件编入 axum）', () => {
    const args = makeCrateFixture({
      memberManifest:
        '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[features]\nhttp = ["dep:axum"]\n\n' +
        '[dependencies]\naxum = "0.8"\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('http 投影 feature 门')
    expect(r.output).toContain('optional')
  })

  it('http feature 未转发 dep:axum → 红（门与依赖面脱钩，门形同虚设）', () => {
    const args = makeCrateFixture({
      memberManifest:
        '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[features]\nhttp = []\n\n' +
        '[dependencies]\naxum = { version = "0.8", optional = true }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('dep:axum')
  })

  it('error.rs impl 摘掉 cfg 门 → 红（删除 cfg 门即变红）', () => {
    const args = makeCrateFixture({
      infraErrorRs:
        'pub struct AppError;\nimpl axum::response::IntoResponse for AppError {\n    fn into_response(self) -> axum::response::Response {}\n}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('http 投影 feature 门')
    expect(r.output).toContain('cfg 门')
  })

  it('impl 前只有普通注释 → 仍红（注释不构成门）', () => {
    const args = makeCrateFixture({
      infraErrorRs:
        'pub struct AppError;\n// 仅注释说明，不构成门\nimpl axum::response::IntoResponse for AppError {\n    fn into_response(self) -> axum::response::Response {}\n}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('http 投影 feature 门')
  })

  it('反向门 #[cfg(not(feature = "http"))] → 红（feature 开启反而消失）', () => {
    const args = makeCrateFixture({
      infraErrorRs:
        'pub struct AppError;\n#[cfg(not(feature = "http"))]\nimpl axum::response::IntoResponse for AppError {\n    fn into_response(self) -> axum::response::Response {}\n}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('http 投影 feature 门')
  })

  it('infra [features] default 含 http → 红（默认 feature 即生产编入）', () => {
    const args = makeCrateFixture({
      memberManifest:
        '[package]\nname = "ledger-infra"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[features]\nhttp = ["dep:axum"]\ndefault = ["http"]\n\n' +
        '[dependencies]\naxum = { version = "0.8", optional = true }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('default 包含 http')
  })

  it('根包 [features] default 含 http → 红', () => {
    const args = makeCrateFixture({ rootFeaturesExtra: 'default = ["http"]' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('default 包含 http')
  })

  it('域侧成员生产依赖对 ledger-infra 启用 http → 红（域侧引入 axum）', () => {
    const args = makeCrateFixture({
      protocolManifest:
        '[package]\nname = "ledger-sync-protocol"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\nledger-infra = { path = "../infra", features = ["http"] }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('http 投影 feature 门')
    expect(r.output).toContain('域侧')
  })

  it('域侧成员直接声明 axum 生产依赖 → 红（不只经 ledger-infra/http 一条路）', () => {
    const args = makeCrateFixture({
      protocolManifest:
        '[package]\nname = "ledger-sync-protocol"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\nledger-infra = { path = "../infra" }\naxum = "0.8"\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('直接声明 axum')
  })

  it('域侧成员仅 dev-dependencies 声明 axum → 绿（测试专用边不在此限）', () => {
    const args = makeCrateFixture({
      protocolManifest:
        '[package]\nname = "ledger-sync-protocol"\nversion = "0.6.0"\nedition = "2024"\n\n' +
        '[dependencies]\nledger-infra = { path = "../infra" }\n\n' +
        '[dev-dependencies]\naxum = "0.8"\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })
})

describe('check-structure INFRA_MODULES 双向全等 + crate 内分层断言（ADR-0111 决策 4 / #1134）', () => {
  it('真实仓库默认通过：INFRA_MODULES 与磁盘模块双向全等 + crate 内块间零未认许引用', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain('双向全等')
    expect(r.output).toContain('块间反向依赖零未认许引用')
  })

  it('新增未登记模块（顶层 .rs 文件）→ 红（清单漂移 fail loud）', () => {
    const args = makeCrateFixture()
    writeFileSync(join(args[1], INFRA_SRC_REL, 'orphan.rs'), STUB)
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('未登记')
    expect(r.output).toContain('orphan.rs')
  })

  it('新增未登记模块（目录型）→ 红（目录型条目覆盖其全部子目录）', () => {
    const args = makeCrateFixture()
    mkdirSync(join(args[1], INFRA_SRC_REL, 'orphan_dir'), { recursive: true })
    writeFileSync(join(args[1], INFRA_SRC_REL, 'orphan_dir', 'mod.rs'), STUB)
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('未登记')
    expect(r.output).toContain('orphan_dir')
  })

  it('crate 根声明文件 lib.rs 登记后缺失 → 红', () => {
    const args = makeCrateFixture()
    rmSync(join(args[1], INFRA_SRC_REL, 'lib.rs'))
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('白名单路径不存在')
    expect(r.output).toContain('lib.rs')
  })

  it('db 引用 boot（非认许边文件）→ 红并定位文件行号', () => {
    const args = makeFixture({
      'db/helper.rs': 'use crate::boot::encryption::probe_file_kind;\npub fn x() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('crate 内反向依赖')
    expect(r.output).toContain('db/helper.rs:1')
  })

  it('db 引用 signals → 红并定位文件行号（shell_support 靶已随 #1108 迁出退役）', () => {
    const args = makeFixture({
      'db/runtime.rs': 'use crate::signals::WriteOp;\npub fn x() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('signals')
  })

  it('db/mod.rs 再导出 shim 合规绿；他文件同引用红', () => {
    const shim = 'pub use crate::boot::{encryption, data_location};\npub fn x() {}\n'
    const green = makeFixture({ 'db/mod.rs': shim })
    expect(run(green).status).toBe(0)

    const bad = makeFixture({ 'db/helper.rs': shim })
    const r = run(bad)
    expect(r.status).toBe(1)
    expect(r.output).toContain('db/helper.rs:1')
  })

  it('boot → db 合法单向 → 绿', () => {
    const args = makeFixture({
      'boot/helper.rs': 'use crate::db::open_connection;\npub fn x() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('注释与字符串中的块路径不误报', () => {
    const args = makeFixture({
      'db/helper.rs': [
        '/// [`crate::boot`] 升顶层（文档注释不算）',
        '// 历史 shell_support 引用已随 #1108 迁出根包',
        'let s = "crate::signals::WriteOp";',
        'pub fn f() {}',
        '',
      ].join('\n'),
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('外挂测试豁免：db/tests/ 引用 boot 不红', () => {
    const args = makeFixture({
      'db/tests/common.rs': 'pub fn s() { crate::boot::encryption::probe_file_kind(); }\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })
})

describe('check-structure TRANSACTION_MODULES 双向全等 + 区级层序（ADR-0113 决策 7 / #1181）', () => {
  // 四条新断言各由负向夹具锚定（ADR-0087 断言强度，断言对准退出码与输出）：
  // ① 双向全等、② 区级层序、③ 模型目录判据各有一枚夹具；删任一条断言须动
  // 脚本（清单外无豁免面），对应夹具转绿 → 该夹具测试失败（CI 红）。
  it('真实仓库默认通过：磁盘模块全部登记 + 区级层序零未认许反向引用（#1182 消除三处反边后认许边归空）', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain('TRANSACTION_MODULES 双向全等')
    expect(r.output).toContain('区级层序零未认许反向引用')
    // 认许边条数自脚本导出清单派生（单一事实源，无双源漂移）
    expect(r.output).toContain(`认许边 ${TRANSACTION_ZONE_ALLOWED_EDGES.length} 条`)
  })

  it('① 磁盘新增未登记模块（顶层 .rs）→ 红（清单漂移 fail loud）', () => {
    const args = makeCrateFixture()
    writeFileSync(join(args[1], TRANSACTION_SRC_REL, 'orphan.rs'), STUB)
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('TRANSACTION_MODULES 未登记')
    expect(r.output).toContain('orphan.rs')
  })

  it('① 磁盘新增未登记模块（目录型）→ 红', () => {
    const args = makeCrateFixture()
    mkdirSync(join(args[1], TRANSACTION_SRC_REL, 'orphan_dir'), { recursive: true })
    writeFileSync(join(args[1], TRANSACTION_SRC_REL, 'orphan_dir', 'helper.rs'), STUB)
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('TRANSACTION_MODULES 未登记')
    expect(r.output).toContain('orphan_dir')
  })

  it('① 仅含测试豁免形态的目录不视为磁盘模块（funding/ / writer/ / tests/ 现行形状绿）', () => {
    const args = makeCrateFixture()
    mkdirSync(join(args[1], TRANSACTION_SRC_REL, 'extra'), { recursive: true })
    writeFileSync(join(args[1], TRANSACTION_SRC_REL, 'extra', 'tests.rs'), STUB)
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('crate 根 lib.rs 是声明与再导出面：跨区再导出不参与区级判向（免登不误报）', () => {
    const args = makeCrateFixture()
    writeFileSync(
      join(args[1], TRANSACTION_SRC_REL, 'lib.rs'),
      'pub use crate::write::writer::NormalizedRow;\npub use crate::read::TransactionView;\npub fn stub() {}\n',
    )
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('② 共享语义引用写路径（认许边之外）→ 红并定位文件行号', () => {
    const args = makeFixture({
      'command/payload.rs': 'use crate::write::writer::NormalizedRow;\npub fn x() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('区级反向依赖')
    expect(r.output).toContain('command/payload.rs:1')
  })

  it('② 共享语义引用接缝（认许边之外）→ 红', () => {
    const args = makeFixture({
      'search_text.rs': 'use crate::seams::merchant::ensure_merchant;\npub fn x() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('区级反向依赖')
  })

  it('② 接缝引用路径区（写 / 读）→ 红', () => {
    const seamToWrite = makeFixture({
      'seams/merchant.rs': 'use crate::write::protocol::create;\npub fn x() {}\n',
    })
    expect(run(seamToWrite).status).toBe(1)
    const seamToRead = makeFixture({
      'seams/investment.rs': 'use crate::read::list_transactions;\npub fn x() {}\n',
    })
    expect(run(seamToRead).status).toBe(1)
  })

  it('② 写读两径互不依赖（双向）→ 红', () => {
    const writeToRead = makeFixture({
      'write/batch.rs': 'use crate::read::search::search_transactions;\npub fn x() {}\n',
    })
    const r = run(writeToRead)
    expect(r.status).toBe(1)
    expect(r.output).toContain('区级反向依赖')
    const readToWrite = makeFixture({
      'read/mod.rs': 'use crate::write::batch::TransactionBatch;\npub fn x() {}\n',
    })
    expect(run(readToWrite).status).toBe(1)
  })

  it('合法层序链：写→接缝→共享语义、读→同区、同区互依 → 绿', () => {
    const args = makeFixture({
      'write/batch.rs':
        'use crate::seams::balance::recalculate;\nuse crate::amount::TransactionKind;\npub fn x() {}\n',
      'read/search.rs':
        'use crate::read::source::list_view;\nuse crate::model::Transaction;\npub fn y() {}\n',
      'write/protocol.rs': 'use crate::write::writer::insert_row;\npub fn z() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('注释与字符串中的跨区路径不误报（掩码边界）', () => {
    const args = makeFixture({
      'search_text.rs': [
        '/// 消费方见 `crate::write::writer` 与 `crate::read`（文档注释不算依赖）',
        '// crate::write::protocol::create',
        'let s = "crate::write::batch::run";',
        'pub fn f() {}',
        '',
      ].join('\n'),
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('花括号列举逐条展开：非首段跨区条目同样命中', () => {
    const args = makeFixture({
      'search_text.rs': 'use crate::{model::Transaction, read::list_view};\npub fn x() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('区级反向依赖')
    expect(r.output).toContain('read')
  })

  it('花括号列举闭括号后文本不吞入：后续枚举变体同名不误报（off-by-one 回归锚，缺它则闭括号扫描失效假绿）', () => {
    const args = makeFixture({
      'search_text.rs':
        'use crate::{model::Transaction};\npub enum E { A, write }\npub fn x() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('外挂测试豁免：write/writer/tests/ 引用读路径不红（ADR-0056 决策 5）', () => {
    const args = makeCrateFixture()
    mkdirSync(join(args[1], TRANSACTION_SRC_REL, 'write', 'writer', 'tests'), { recursive: true })
    writeFileSync(
      join(args[1], TRANSACTION_SRC_REL, 'write', 'writer', 'tests', 'fixture.rs'),
      'use crate::read::list_view;\npub fn s() {}\n',
    )
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('③ 模型目录成员的 glob 聚合 → 红（判据扩到目录形态，模型目录化不静默失靶）', () => {
    const args = makeFixture({
      'test_support/model/price.rs': 'pub use crate::test_support::types::*;\npub fn x() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('域模型文件内 glob 聚合')
    expect(r.output).toContain('test_support/model/price.rs')
  })

  it('③ 模型文件名判据不回退：model.rs 内 glob 仍红（文件形态先行例）', () => {
    const args = makeFixture({
      'test_support/model.rs': 'pub use crate::test_support::types::*;\npub fn x() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('域模型文件内 glob 聚合')
  })

  it('③ 模型目录成员不含 glob → 绿（判据只拦聚合，不拦逐类型再导出与类型定义）', () => {
    const args = makeFixture({
      'test_support/model/price.rs':
        'pub struct Price;\npub use crate::test_support::types::{Price as ItemPrice};\npub fn x() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })
})

describe('maskNonCode 双源防漂移语料（issue #1433）', () => {
  // 与 Rust 侧唯一实现（src-tauri/src/test_support/scan.rs 的 mask_non_code）
  // 消费同一夹具：语料输入 + 期望输出双文件共享，任一侧单独改词法规则即
  // 本测或 Rust 语料测试红。（vitest 下取进程 cwd = 仓库根定位夹具，同上款先例）
  const corpusPath = join(process.cwd(), 'scripts', 'fixtures', 'rust-mask-corpus.rs')
  const expectedPath = join(process.cwd(), 'scripts', 'fixtures', 'rust-mask-corpus.expected.txt')

  it('掩码输出与共享语料期望全等（与 Rust 侧 mask_non_code 同规）', () => {
    const corpus = readFileSync(corpusPath, 'utf8')
    const expected = readFileSync(expectedPath, 'utf8')
    expect(maskNonCode(corpus)).toBe(expected)
  })

  it('keepLiterals=true 只掩注释、保留字符串字面量（TS 侧扩展形态，语料外的直接断言）', () => {
    const src = '// comment\nlet s = "keep";\n'
    expect(maskNonCode(src, true)).toBe('          \nlet s = "keep";\n')
  })
})
