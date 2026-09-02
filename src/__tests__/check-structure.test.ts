import { afterAll, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import { WHITELIST } from '../../scripts/check-structure.js'

// 被测对象是仓库工具脚本 scripts/check-structure.js（结构守门，ADR-0056）。
// 按测试决策只测外部可观察结果——进程退出码与输出，不测内部函数；
// 通过位置参数把扫描目标指向临时夹具目录。
// 夹具白名单清单自脚本导出的 WHITELIST 派生（单一事实源，无双源漂移）；
// （vitest 转换后 import.meta.url 非 file: scheme，取进程 cwd = 仓库根定位脚本）
const script = join(process.cwd(), 'scripts', 'check-structure.js')

interface RunResult {
  status: number
  output: string
}

function run(args: string[]): RunResult {
  const r = spawnSync(process.execPath, [script, ...args], { encoding: 'utf8' })
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

// 夹具用现存的壳层引用形态（交易壳层 `*_internal`）：商户域 #400 归位后原
// `commands::merchants::list_merchants_internal` 已不存在，夹具文本与实际结构保持一致。
const shellUse = 'use crate::commands::transactions::create_transaction_internal;\npub fn x() {}\n'

describe('check-structure（结构守门）', () => {
  it('真实仓库默认通过：白名单对壳层零依赖', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain('零依赖')
    expect(r.output).toContain('域目录 6')
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
