import { afterAll, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

// 被测对象是仓库工具脚本 scripts/check-commands.js（命令注册一致性校验）。
// 按测试决策只测外部可观察结果——进程退出码与输出，不测内部函数；
// 通过位置参数把扫描目标指向临时夹具目录。
// （vitest 转换后 import.meta.url 非 file: scheme，取进程 cwd = 仓库根定位脚本）
const script = join(process.cwd(), 'scripts', 'check-commands.js')

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

/** 建临时夹具：commands 命令目录 + api.ts 调用面文件，返回脚本参数 */
function makeFixture(commands: Record<string, string>, apiTs: string): string[] {
  const dir = mkdtempSync(join(tmpdir(), 'check-commands-'))
  tempDirs.push(dir)
  const cmdsDir = join(dir, 'commands')
  for (const [relPath, content] of Object.entries(commands)) {
    const file = join(cmdsDir, relPath)
    mkdirSync(join(file, '..'), { recursive: true })
    writeFileSync(file, content)
  }
  const apiFile = join(dir, 'api.ts')
  writeFileSync(apiFile, apiTs)
  return [cmdsDir, apiFile]
}

const cmd = (name: string) => `#[tauri::command]\npub fn ${name}(db: DbState) -> String {\n    todo!()\n}\n`

describe('check-commands（命令注册一致性校验）', () => {
  it('真实仓库默认通过：Rust 注解命令集与 TS 调用面双向全等', () => {
    const r = run([])
    expect(r.status).toBe(0)
    expect(r.output).toContain('双向全等')
  })

  it('夹具两侧一致时通过', () => {
    const args = makeFixture(
      {
        'alpha.rs': cmd('alpha_one') + cmd('alpha_two'),
        'beta/mod.rs': cmd('beta_one'),
      },
      [
        "import { invoke } from '@tauri-apps/api/core'",
        'export const api = {',
        "  a: () => invoke<void>('alpha_one'),",
        "  b: () => invoke<void>('alpha_two'),",
        "  c: () => invoke<string>('beta_one'),",
        '}',
        '',
      ].join('\n'),
    )
    const r = run(args)
    expect(r.status).toBe(0)
    expect(r.output).toContain('双向全等')
  })

  it('TS 缺方法（Rust 有 TS 无）→ 失败并列出差异', () => {
    const args = makeFixture(
      { 'alpha.rs': cmd('alpha_one') + cmd('alpha_two') },
      "invoke<void>('alpha_one')\n",
    )
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('TS 调用面缺方法')
    expect(r.output).toContain('alpha_two')
    expect(r.output).not.toContain('- alpha_one\n')
  })

  it('TS 调用不存在的命令 → 失败并列出差异', () => {
    const args = makeFixture(
      { 'alpha.rs': cmd('alpha_one') },
      "invoke<void>('alpha_one')\ninvoke<void>('ghost_cmd')\n",
    )
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toContain('Rust 无此命令')
    expect(r.output).toContain('ghost_cmd')
  })

  it('注解后不是 fn 定义（扫描器不认识的形态）→ 报扫描边界错误', () => {
    const args = makeFixture(
      {
        'alpha.rs':
          '#[tauri::command]\n#[cfg(target_os = "macos")]\npub fn alpha_one() {}\n',
      },
      "invoke<void>('alpha_one')\n",
    )
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toMatch(/扫描器/)
    expect(r.output).toContain('alpha.rs')
  })

  it('命令名重复定义 → 失败', () => {
    const args = makeFixture(
      { 'alpha.rs': cmd('alpha_one'), 'beta/mod.rs': cmd('alpha_one') },
      "invoke<void>('alpha_one')\n",
    )
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toMatch(/重复/)
    expect(r.output).toContain('alpha_one')
  })

  it('空集（两侧皆扫不到命令）→ 拒绝以「0 ↔ 0」假绿通过', () => {
    const args = makeFixture({ 'alpha.rs': '// 无命令的文件' }, 'export const api = {}\n')
    const r = run(args)
    expect(r.status).toBe(1)
    expect(r.output).toMatch(/未在命令目录扫描到任何/)
    expect(r.output).toMatch(/未在 TS 调用面扫描到任何/)
  })
})
