import { afterAll, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

// 被测对象是仓库工具脚本 scripts/check-commands.ts（命令注册一致性校验：命令名腿 + 参数键名腿）。
// 脚本以 Bun 运行时执行（ADR-0083）：spawnSync('bun') 与门槛调用同款，测的就是门槛路径。
// 按测试决策只测外部可观察结果——进程退出码与输出，不测内部函数；
// 通过位置参数把扫描目标指向临时夹具目录。
// （vitest 转换后 import.meta.url 非 file: scheme，取进程 cwd = 仓库根定位脚本）
const script = join(process.cwd(), 'scripts', 'check-commands.ts')

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

// 注入参数用真实形态（State<'_, DbState>）：db 是 Tauri 注入参数，不进 invoke 键集（#1398）
const cmd = (name: string) => `#[tauri::command]\npub fn ${name}(db: State<'_, DbState>) -> String {\n    todo!()\n}\n`

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

  describe('参数键名腿（issue #1398，#588 类回归根治）', () => {
    const topCmd =
      "#[tauri::command]\npub fn top_report(db: State<'_, DbState>, top_n: Option<i64>) -> String {\n    todo!()\n}\n"

    it('参数键名全等（含属性简写、多行对象、snake→camel 转换、注入参数不进键集）→ 通过', () => {
      const args = makeFixture(
        {
          'items.rs':
            "#[tauri::command]\npub fn calc_cost(db: State<'_, DbState>, id: String, reference_date: Option<String>) -> String {\n    todo!()\n}\n",
        },
        [
          "import { invoke } from '@tauri-apps/api/core'",
          'export const api = {',
          '  calc: (id: string, referenceDate?: string | null) =>',
          "    invoke<string>('calc_cost', {",
          '      id,',
          '      referenceDate: referenceDate ?? null,',
          '    }),',
          '}',
          '',
        ].join('\n'),
      )
      const r = run(args)
      expect(r.status).toBe(0)
      expect(r.output).toMatch(/参数键名/)
    })

    it('top_n 形态失配（Rust top_n ↔ TS 键 top_n）→ 失败且差异列出该键', () => {
      const args = makeFixture(
        { 'reports.rs': topCmd },
        "invoke<string>('top_report', { top_n: 5 })\n",
      )
      const r = run(args)
      expect(r.status).toBe(1)
      expect(r.output).toContain('topN') // 期望键（Rust 参数名转换后）
      expect(r.output).toContain('top_n') // 失配的实际键
    })

    it('实参缺键（TS 漏传可选参数）→ 失败并列出缺失键', () => {
      const args = makeFixture(
        { 'reports.rs': topCmd },
        "invoke<string>('top_report')\n",
      )
      const r = run(args)
      expect(r.status).toBe(1)
      expect(r.output).toContain('topN')
    })

    it('同命令多调用点键集不一致 → 失败', () => {
      const args = makeFixture(
        {
          'reports.rs':
            "#[tauri::command]\npub fn ms(db: State<'_, DbState>, year: i64, from: Option<String>, to: Option<String>) -> String {\n    todo!()\n}\n",
        },
        [
          "invoke<string>('ms', { year: 2026, from: null, to: null })",
          "invoke<string>('ms', { year: 2026, from: null })",
          '',
        ].join('\n'),
      )
      const r = run(args)
      expect(r.status).toBe(1)
      expect(r.output).toMatch(/键集不同/)
      expect(r.output).toContain('ms')
    })

    it('含数字段转换与 Tauri 绑定一致（s3_bucket → s3Bucket，数字不成词界）→ 通过', () => {
      const args = makeFixture(
        {
          'sync.rs':
            "#[tauri::command]\npub fn s3_report(db: State<'_, DbState>, s3_bucket: Option<String>) -> String {\n    todo!()\n}\n",
        },
        "invoke<string>('s3_report', { s3Bucket: 'demo' })\n",
      )
      const r = run(args)
      expect(r.status).toBe(0)
    })

    it('实参展开语法（...）→ fail loud 拒绝', () => {
      const args = makeFixture(
        { 'reports.rs': topCmd },
        "invoke<string>('top_report', { topN: 5, ...rest })\n",
      )
      const r = run(args)
      expect(r.status).toBe(1)
      expect(r.output).toMatch(/展开/)
    })

    it('实参计算键（[...]）→ fail loud 拒绝', () => {
      const args = makeFixture(
        { 'reports.rs': topCmd },
        "invoke<string>('top_report', { ['topN']: 5 })\n",
      )
      const r = run(args)
      expect(r.status).toBe(1)
      expect(r.output).toMatch(/计算键/)
    })

    it('实参非对象字面量（变量透传）→ fail loud 拒绝', () => {
      const args = makeFixture(
        { 'reports.rs': topCmd },
        "invoke<string>('top_report', someArgs)\n",
      )
      const r = run(args)
      expect(r.status).toBe(1)
      expect(r.output).toMatch(/不是对象字面量/)
    })
  })
})
