import { afterAll, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { WHITELIST, LAYER } from '../../scripts/check-structure.ts'

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
 * 建临时夹具：按脚本导出的 WHITELIST 生成全部条目（目录 → mod.rs，文件 → 同名文件），
 * 再按 overrides 追加/覆盖文件。返回脚本参数（夹具 src 目录）。
 */
function makeFixture(overrides: Record<string, string> = {}): string[] {
  const src = mkdtempSync(join(tmpdir(), 'check-structure-'))
  tempDirs.push(src)
  for (const { path } of WHITELIST) {
    const abs = join(src, path)
    if (path.endsWith('.rs')) {
      mkdirSync(join(abs, '..'), { recursive: true })
      writeFileSync(abs, STUB)
    } else {
      mkdirSync(abs, { recursive: true })
      writeFileSync(join(abs, 'mod.rs'), STUB)
    }
  }
  for (const [relPath, content] of Object.entries(overrides)) {
    const file = join(src, relPath)
    mkdirSync(join(file, '..'), { recursive: true })
    writeFileSync(file, content)
  }
  return [src]
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
    rmSync(join(args[0], 'db'), { recursive: true, force: true })
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
  /** 与真实树 db/mod.rs after_commit 同形的夹具文本（ADR-0032 置脏单点，认许边原型） */
  const afterCommitShape = [
    'pub fn write<T>(f: impl FnOnce() -> T) -> T { f() }',
    'fn after_commit(conn: &Connection) {',
    '    if let Err(e) = crate::backup::mark_dirty(conn) {',
    '        tracing::warn!(error = %e, "写库成功但置脏失败（忽略）");',
    '    }',
    '    let dir = crate::backup::shared_prefs().snapshot_dir();',
    '    crate::backup::run_due_backup(',
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

  it('内联全限定路径（crate::backup::x() 形态）同样识别 → 红', () => {
    const args = makeFixture({
      'db/helper.rs': 'pub fn y() { crate::backup::mark_dirty(); }\n',
    })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('引用域目录 backup')
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
    const args = makeFixture({ 'db/helper.rs': 'use crate::backup;\npub fn z() {}\n' })
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('引用域目录 backup')
  })

  it('std::sync 等同名路径不误报（crate 根前缀限定边界）', () => {
    const args = makeFixture({
      'db/helper.rs': 'use std::sync::{Arc, Mutex};\npub fn z(a: Arc<Mutex<u8>>) {}\n',
    })
    const r = run(args)
    expect(r.status).toBe(0)
  })

  it('域目录条目内的域间横向引用不红（扫描范围仅基础设施条目，ADR-0071 决策 5）', () => {
    const args = makeFixture({
      'transaction/writer.rs': 'use crate::accounts::Account;\npub fn x() {}\n',
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

  it('认许边精确匹配：db/mod.rs→backup 绿；同文件他域或他文件同域仍红', () => {
    const green = makeFixture({ 'db/mod.rs': afterCommitShape })
    expect(run(green).status).toBe(0)

    const otherDomain = makeFixture({
      'db/mod.rs': afterCommitShape + 'use crate::accounts::Account;\n',
    })
    const r1 = run(otherDomain)
    expect(r1.status).toBe(1)
    expect(r1.output).toContain('引用域目录 accounts')

    const otherFile = makeFixture({
      'db/helper.rs': 'use crate::backup::mark_dirty;\n',
    })
    const r2 = run(otherFile)
    expect(r2.status).toBe(1)
    expect(r2.output).toContain('db/helper.rs:1')
  })

  it('真实仓库默认通过：基础设施→域零未认许引用（认许边留痕于脚本）', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain('认许边 1 条')
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
