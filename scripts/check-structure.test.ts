import { afterAll, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  BACKUP_MODULES,
  BACKUP_SRC_REL,
  CRATES,
  INFRA_MODULES,
  INFRA_SRC_REL,
  PROTOCOL_MODULES,
  PROTOCOL_SRC_REL,
  WHITELIST,
  LAYER,
} from '../scripts/check-structure.ts'

// 被测对象是仓库工具脚本 scripts/check-structure.ts（结构守门，ADR-0056）。
// 脚本以 Bun 运行时执行（ADR-0083）：spawnSync('bun') 与门槛调用同款，测的就是门槛路径。
// 按测试决策只测外部可观察结果——进程退出码与输出，不测内部函数；
// 通过位置参数把扫描目标指向临时夹具目录。
// 夹具白名单清单自脚本导出的 WHITELIST 派生（单一事实源，无双源漂移）；
// （vitest 转换后 import.meta.url 非 file: scheme，取进程 cwd = 仓库根定位脚本）
const script = join(process.cwd(), 'scripts', 'check-structure.ts')

interface RunResult {
  status: number
  output: string
}

function run(args: string[]): RunResult {
  const r = spawnSync('bun', [script, ...args], { encoding: 'utf8' })
  return { status: r.status ?? -1, output: (r.stdout ?? '') + (r.stderr ?? '') }
}

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

/**
 * 写覆盖文件：按路径首段归位——基础设施模块（`db/…` / `error.rs` / …）落
 * `<srcTauri>/crates/infra/src`，备份域 crate 模块（`auto.rs` / `engine.rs`，#1091）
 * 落 `<srcTauri>/crates/backup/src`，其余（域目录、壳层 `commands/` 等）落 `<srcTauri>/src`。
 */
function placeOverride(srcTauri: string, relPath: string, content: string): void {
  const base = isInfraModulePath(relPath)
    ? join(srcTauri, INFRA_SRC_REL)
    : isBackupModulePath(relPath)
      ? join(srcTauri, BACKUP_SRC_REL)
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
    const args = makeFixture({ 'item/crud.rs': shellUse })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('反向依赖')
    expect(r.output).toContain('item/crud.rs:1')
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
      'item/cost.rs': [
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
      'item/tests.rs': shellUse,
      'item/tests/scaffold.rs': shellUse,
      'transaction/writer/tests/fixture.rs': shellUse,
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
    rmSync(join(args[0], 'item'), { recursive: true, force: true })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('白名单路径不存在')
    expect(r.output).toContain('item')
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
    rmSync(join(args[0], 'item', 'mod.rs'))
    writeFileSync(join(args[0], 'item', 'tests.rs'), shellUse) // 只剩豁免形态
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
  // #1091 起备份域拆独立 crate（crate 名直引 `ledger_backup::` 不再经域目录路径
  // 扫描，反向引用由 cargo 依赖图拒绝），负向夹具改以 accounts 为靶域保留同形
  // ——「基础设施→域生产挂载点不得复活」的钉子不变。
  const afterCommitShape = [
    'pub fn write<T>(f: impl FnOnce() -> T) -> T { f() }',
    'fn after_commit(conn: &Connection) {',
    '    if let Err(e) = crate::accounts::mark_dirty(conn) {',
    '        tracing::warn!(error = %e, "写库成功但置脏失败（忽略）");',
    '    }',
    '    let dir = crate::accounts::shared_prefs().snapshot_dir();',
    '    crate::accounts::run_due_backup(',
    '        conn,',
    '        dir.as_deref(),',
    '    );',
    '}',
    '',
  ].join('\n')

  it('基础设施文件 use 域模块 → 红', () => {
    const args = makeFixture({ 'db/helper.rs': 'use crate::accounts::Account;\npub fn x() {}\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('引用域目录')
    expect(r.output).toContain('db/helper.rs:1')
  })

  it('内联全限定路径（crate::accounts::x() 形态）同样识别 → 红', () => {
    const args = makeFixture({
      'db/helper.rs': 'pub fn y() { crate::accounts::mark_dirty(); }\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('引用域目录 accounts')
    expect(r.output).toContain('db/helper.rs:1')
  })

  it('tauri_app_lib:: 前缀与 use as 别名引入同样识别 → 红', () => {
    const args = makeFixture({
      'events.rs': 'use tauri_app_lib::dashboard::DashboardOverview;\npub fn x() {}\n',
      'settings.rs': 'use crate::accounts as acct;\npub fn y() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('引用域目录 dashboard')
    expect(r.output).toContain('引用域目录 accounts')
  })

  it('模块自身导入（use crate::<域>;）同样识别 → 红', () => {
    const args = makeFixture({ 'db/helper.rs': 'use crate::accounts;\npub fn z() {}\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('引用域目录 accounts')
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
    // 本用例改用非禁边对（item→backup）钉住「域间横向引用本身不在 infra 扫描范围」。
    const args = makeFixture({
      'item/crud.rs': 'use crate::backup::AutoBackupState;\npub fn x() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('注释与字符串中的域路径不误报（掩码边界）', () => {
    const args = makeFixture({
      'db/helper.rs': [
        '/// 提交点由 [`crate::backup::run_due_backup`] 统一门禁（文档注释不算引用）',
        '// 见 crate::accounts::Account 说明',
        'let s = "crate::backup::mark_dirty";',
        'let re = r#"crate::sync::fetch"#;',
        'pub fn f() {}',
        '',
      ].join('\n'),
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('外挂测试豁免不变：tests.rs 与 tests/ 目录引用域不红（ADR-0056 决策 5）', () => {
    const args = makeFixture({
      'db/tests.rs': 'use crate::accounts::Account;\n',
      'db/tests/common.rs': 'pub fn s() -> crate::backup::AutoBackupState { todo!() }\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('生产挂载点已反转：db/mod.rs 直调域副作用 → 红（认许边不再含该条）', () => {
    const args = makeFixture({ 'db/mod.rs': afterCommitShape })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('引用域目录 accounts')
    expect(r.output).toContain('db/mod.rs:3')
  })

  it('认许边精确匹配：settings.rs→test_support 绿；同文件他域或他文件同域仍红', () => {
    const green = makeFixture({
      'settings.rs': 'use tauri_app_lib::test_support::open;\npub fn x() {}\n',
    })
    expect(run(green).status).toBe(0)

    const otherDomain = makeFixture({
      'settings.rs':
        'use tauri_app_lib::test_support::open;\nuse crate::accounts::Account;\npub fn x() {}\n',
    })
    const r1 = run(otherDomain)
    expect(r1.status).toBe(1)
    expect(r1.output).toContain('引用域目录 accounts')

    const otherFile = makeFixture({
      'db/helper.rs': 'use crate::accounts::Account;\n',
    })
    const r2 = run(otherFile)
    expect(r2.status).toBe(1)
    expect(r2.output).toContain('db/helper.rs:1')
  })

  it('真实仓库默认通过：基础设施→域零未认许引用（认许边留痕于脚本）', () => {
    const r = run([])
    expect(r.status).toBe(0)
    // 生产挂载点 0（#1088 注册点反转消除 db/mod.rs→backup）+ settings/logger/write_entry/read_entry→test_support（ADR-0084，#758）
    expect(r.output).toContain('认许边 4 条')
  })
})

describe('check-structure 业务域→同步域零容忍（ADR-0101 决策 4b / #1089 收紧）', () => {
  it('业务域引用同步域内部件（engine::/ops::/model::…）→ 红并定位文件行号', () => {
    const args = makeFixture({
      'scheduled_transactions/command.rs': 'use crate::sync_engine::engine::ReplayEffect;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('scheduled_transactions/command.rs:1')
    expect(r.output).toContain('sync_engine::engine')
  })

  it('契约模块与原白名单根符号亦红（#1089 零容忍：协议面下放协议 crate）', () => {
    const args = makeFixture({
      'item/command.rs': [
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
    expect(r.output).toContain('item/command.rs:1')
  })

  it('根花括号列举夹带任一符号 → 红（零容忍逐条判定）', () => {
    const args = makeFixture({
      'item/command.rs': 'use crate::sync_engine::{DomainCommand, model::SyncOp};\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('model')
  })

  it('根 glob 引入 → 红（零容忍）', () => {
    const args = makeFixture({ 'item/command.rs': 'use crate::sync_engine::*;\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
  })

  it('根别名引入（use crate::sync_engine as se）→ 红（堵别名盲区）', () => {
    const args = makeFixture({
      'item/command.rs': 'use crate::sync_engine as se;\npub fn x() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('业务域引用同步域')
    expect(r.output).toContain('item/command.rs:1')
  })

  it('同步域自身与测试支持域不参与（作用域边界）', () => {
    const args = makeFixture({
      'sync_engine/engine.rs': 'use crate::sync_engine::ops::insert_row;\n',
      'test_support/channel.rs': 'use crate::sync_engine::model::SyncOp;\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('注释与字符串中的同步域内部路径不误报（掩码边界）', () => {
    const args = makeFixture({
      'item/command.rs': [
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
  it('transaction 引用 accounts（余额重算旧形态）→ 红并定位文件行号', () => {
    const args = makeFixture({
      'transaction/writer.rs':
        'use crate::accounts::balance::{affected_accounts, refresh_account_balances};\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('域间禁边')
    expect(r.output).toContain('transaction/writer.rs:1')
    expect(r.output).toContain('accounts')
  })

  it('transaction 引用 scheduled_transactions（计划反查旧形态）→ 红', () => {
    const args = makeFixture({
      'transaction/read.rs':
        'let rows = crate::scheduled_transactions::source_display_by_transaction_ids(conn, &ids)?;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('域间禁边')
    expect(r.output).toContain('transaction/read.rs:1')
  })

  it('scheduled_transactions 引用 backup（置脏旧形态）→ 红', () => {
    const args = makeFixture({
      'scheduled_transactions/auto_run.rs': 'crate::backup::mark_dirty(conn);\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('域间禁边')
    expect(r.output).toContain('scheduled_transactions/auto_run.rs:1')
  })

  it('scheduled_transactions 引用 ledger_backup::（crate 名直引，#1091）→ 红', () => {
    // #1091 起备份域实现住 ledger-backup crate：除再导出面（crate::backup /
    // tauri_app_lib::backup）外，crate 名直引前缀同属禁令形态（extraPattern 并扫）。
    const args = makeFixture({
      'scheduled_transactions/auto_run.rs': 'ledger_backup::mark_dirty(conn);\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('域间禁边')
    expect(r.output).toContain('scheduled_transactions/auto_run.rs:1')
  })

  it('根花括号列举首段同样识别 → 红（与 infra→域扫描同款形态）', () => {
    const args = makeFixture({
      'transaction/read.rs': 'use crate::{accounts::balance::compute_balance, db::query::query_all};\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('域间禁边')
  })

  it('认许边精确匹配：transaction/funding.rs 的 AccountType 消费绿；同引用挪到他文件仍红', () => {
    const green = makeFixture({
      'transaction/funding.rs': 'use crate::accounts::AccountType;\npub fn x() {}\n',
    })
    expect(run(green).status).toBe(0)

    const moved = makeFixture({
      'transaction/writer.rs': 'use crate::accounts::AccountType;\npub fn x() {}\n',
    })
    const r = run(moved)
    expect(r.status).toBe(1)
    expect(r.output).toContain('域间禁边')
    expect(r.output).toContain('transaction/writer.rs:1')
  })

  it('方向性：禁边反向（accounts → transaction）不红——口径真源单向边合法', () => {
    const args = makeFixture({
      'accounts/core.rs': 'use crate::transaction::amount::account_flow_expr;\npub fn x() {}\n',
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
      'transaction/tests/balance_cache.rs': 'use crate::accounts::balance::compute_balance;\n',
      'scheduled_transactions/tests/auto_run.rs': 'crate::backup::get_state(&conn);\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('真实仓库默认通过：域间禁边零未认许引用（认许边留痕于脚本）', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain('域间禁边 3 对零未认许引用（认许边 1 条，#1090 接缝反转）')
  })

  it('删除即变红：禁边规则逐对生效，删对后同夹具转绿（对保护不假绿）', () => {
    // 同一夹具在禁边在位时红；规则对逐条生效，删除规则须动脚本（清单外无豁免面）。
    const fixture = {
      'scheduled_transactions/auto_run.rs': 'crate::backup::mark_dirty(conn);\n',
    }
    expect(run(makeFixture(fixture)).status).toBe(1)
  })
})

describe('check-structure 模型域化禁令（ADR-0059 决策 6 / #424 T7 收口）', () => {
  it('规则①：crate::models 全局模型路径残留 → 红', () => {
    const args = makeFixture({ 'item/crud.rs': 'use crate::models::Transaction;\npub fn x() {}\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('全局模型路径残留')
    expect(r.output).toContain('item/crud.rs:1')
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
      'item/cost.rs': [
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
      'item/tests.rs': 'use crate::models::Transaction;\n',
      'item/tests/scaffold.rs': 'use tauri_app_lib::models::Transaction;\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('规则②：域接缝 glob 再导出 pub use model::* → 红', () => {
    const args = makeFixture({ 'item/mod.rs': 'mod model;\npub use model::*;\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('glob 再导出')
    expect(r.output).toContain('item/mod.rs:2')
  })

  it('规则②：跨域拍平形态 pub use crate::x::model::* → 红', () => {
    const args = makeFixture({
      'item/mod.rs': 'pub use crate::transaction::model::*;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('glob 再导出')
  })

  it('规则②：旧全局目录同名形态 pub use models::* → 红', () => {
    const args = makeFixture({ 'item/mod.rs': 'pub use models::*;\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('glob 再导出')
  })

  it('规则②：域模型文件内 glob 聚合 pub use xxx::* → 红', () => {
    const args = makeFixture({ 'item/model.rs': 'pub use super::crud::*;\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('glob 聚合')
    expect(r.output).toContain('item/model.rs:1')
  })

  it('规则②：逐类型再导出与域内私有 glob 引用合规 → 绿', () => {
    const args = makeFixture({
      'item/mod.rs': 'mod model;\npub use model::{Item, ItemInput};\n',
      'item/behavior.rs': 'use super::model::*;\npub fn x() {}\n',
      'item/model.rs': 'pub struct Item;\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })
})

describe('check-structure 原生事务语句禁令（issue #1014 / #1003 定案 7）', () => {
  it('产品代码手写 BEGIN → 红并定位文件行号', () => {
    const args = makeFixture({
      'accounts/core.rs': 'pub fn f(conn: &Connection) {\n    conn.execute("BEGIN", []);\n}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('原生事务语句')
    expect(r.output).toContain('accounts/core.rs:2')
    expect(r.output).toContain('db/tx_scope.rs')
  })

  it('COMMIT / ROLLBACK 手写同样识别 → 红', () => {
    const args = makeFixture({
      'transaction/batch.rs': 'pub fn g(conn: &Connection) {\n    conn.execute("COMMIT", []);\n}\n',
      'scheduled_transactions/engine.rs':
        'pub fn h(conn: &Connection) {\n    conn.execute("ROLLBACK", []);\n}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('transaction/batch.rs:2')
    expect(r.output).toContain('scheduled_transactions/engine.rs:2')
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
      'accounts/core.rs': [
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
  /** 覆盖 `crates/infra/src/lib.rs` 内容（test_utils cfg 门负向夹具） */
  infraLibRs?: string
  /** 覆盖根包 `src/lib.rs` 内容（test_utils 再导出 cfg 门负向夹具） */
  rootLibRs?: string
  /** 覆盖根包 `[dependencies]` 的 ledger-infra 行（生产依赖接线负向夹具） */
  rootInfraProdDep?: string
  /** 追加到根包 `[features]` 段的原文行（default feature 负向夹具） */
  rootFeaturesExtra?: string
  checkSh?: string
  testSh?: string
  lintFixSh?: string
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
  mkdirSync(join(srcTauri, 'src'), { recursive: true })
  writeFileSync(
    join(srcTauri, 'src', 'lib.rs'),
    overrides.rootLibRs ??
      '#[cfg(any(test, feature = "test-utils"))]\n' +
        '#[doc(hidden)]\n' +
        'pub use ledger_infra::test_utils;\n',
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

  // 同步协议 crate（#1089）：夹具与真实仓库同形——成员目录 + 门禁继承。
  mkdirSync(join(srcTauri, 'crates', 'sync-protocol', 'src'), { recursive: true })
  writeFileSync(
    join(srcTauri, 'crates', 'sync-protocol', 'Cargo.toml'),
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
        '[dev-dependencies]\ntauri-app = { path = "../.." }\n\n[lints]\nworkspace = true\n',
    })
    const r = run(args)
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

  it('根包再导出摘掉 cfg 门 → 红（生产构建会解析失败即变红）', () => {
    const args = makeCrateFixture({
      rootLibRs: '#[doc(hidden)]\npub use ledger_infra::test_utils;\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('test_utils 生产编译门')
    expect(r.output).toContain('lib.rs')
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

  it('db 引用 shell_support / signals → 红', () => {
    const args = makeFixture({
      'db/runtime.rs': 'use crate::shell_support::write_entry;\nuse crate::signals::WriteOp;\npub fn x() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('shell_support')
    expect(r.output).toContain('signals')
  })

  it('boot 引用 shell_support → 红并定位文件行号', () => {
    const args = makeFixture({
      'boot/helper.rs': 'use crate::shell_support::logger;\npub fn x() {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('crate 内反向依赖')
    expect(r.output).toContain('boot/helper.rs:1')
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
        '// crate::shell_support::write_entry',
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
