import { afterAll, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { REFERENCE_DEFAULTS } from './helpers/reference-stubs'

// 被测对象是仓库工具脚本 scripts/check-test-stubs.js（参考数据测试桩守门，issue #725）。
// 按测试决策只测外部可观察结果——进程退出码与输出，不测内部函数；
// 通过位置参数把扫描目标指向临时夹具目录。
// 夹具命令清单自助手导出的 REFERENCE_DEFAULTS 派生（单一事实源，无双源漂移）；
// （vitest 转换后 import.meta.url 非 file: scheme，取进程 cwd = 仓库根定位脚本）
const script = join(process.cwd(), 'scripts', 'check-test-stubs.js')

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

/** 助手夹具：登记处命令清单由真实 REFERENCE_DEFAULTS 派生（可加自定义命令）。 */
function helperFixture(extraCommands: string[] = []): string {
  const lines = [
    "// 测试夹具：参考数据桩助手（形状与 src/__tests__/helpers/reference-stubs.ts 同构）",
    'export const REFERENCE_DEFAULTS: Record<string, unknown> = {',
    ...Object.keys(REFERENCE_DEFAULTS).map((cmd) => `  ${cmd}: [],`),
    ...extraCommands.map((cmd) => `  ${cmd}: [],`),
    '}',
  ]
  return lines.join('\n') + '\n'
}

/** 建夹具目录：helpers/reference-stubs.ts + overrides 里的测试文件。返回目录路径。 */
function makeFixture(files: Record<string, string>, extraCommands: string[] = []): string {
  const dir = mkdtempSync(join(tmpdir(), 'check-test-stubs-'))
  tempDirs.push(dir)
  mkdirSync(join(dir, 'helpers'), { recursive: true })
  writeFileSync(join(dir, 'helpers', 'reference-stubs.ts'), helperFixture(extraCommands))
  for (const [name, content] of Object.entries(files)) {
    const p = join(dir, name)
    mkdirSync(join(p, '..'), { recursive: true })
    writeFileSync(p, content)
  }
  return dir
}

const CLEAN_TEST = `import { stubReferenceInvoke } from './helpers/reference-stubs'
// 领域数据命令的手搓 if 链不在守门范围
mockInvoke.mockImplementation((cmd: string) => {
  if (cmd === 'list_transactions') return Promise.resolve([])
  return Promise.reject(new Error('unexpected invoke'))
})
stubReferenceInvoke({ list_transactions: [] })
`

describe('check-test-stubs', () => {
  it('全仓无手搓参考数据桩 if 链时通过（退出码 0）', () => {
    const dir = makeFixture({ 'SomeView.test.ts': CLEAN_TEST })
    const r = run([dir])
    expect(r.status).toBe(0)
    expect(r.output).toContain('参考数据测试桩守门')
  })

  it('手搓 if 链桩参考命令即红，逐处报文件与命令', () => {
    const dir = makeFixture({
      'Bad.test.ts': `mockInvoke.mockImplementation((cmd: string) => {
  if (cmd === 'list_insurers') return Promise.resolve([])
  if (cmd === 'list_transactions') return Promise.resolve([])
  return Promise.reject(new Error('unexpected invoke'))
})`,
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain('Bad.test.ts')
    expect(r.output).toContain('list_insurers')
  })

  it('三元与 switch 形态的手搓桩同样识别', () => {
    const dir = makeFixture({
      'Ternary.test.ts': `const impl = (cmd: string) => (cmd === 'list_currencies' ? Promise.resolve([]) : Promise.reject(new Error('x')))
`,
      'Switch.test.ts': `function impl(cmd: string) {
  switch (cmd) {
    case 'list_accounts':
      return Promise.resolve([])
    default:
      return Promise.reject(new Error('x'))
  }
}
`,
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain('Ternary.test.ts')
    expect(r.output).toContain('list_currencies')
    expect(r.output).toContain('Switch.test.ts')
    expect(r.output).toContain('list_accounts')
  })

  it('领域数据命令（不在登记处）的手搓 if 链不红', () => {
    const dir = makeFixture({
      'Domain.test.ts': `mockInvoke.mockImplementation((cmd: string) => {
  if (cmd === 'list_policies') return Promise.resolve([])
  return Promise.reject(new Error('unexpected invoke'))
})`,
    })
    expect(run([dir]).status).toBe(0)
  })

  it('命令清单以助手登记处为单一来源：新增参考命令自动纳管', () => {
    const dir = makeFixture(
      {
        'Future.test.ts': `// 新增参考表后有人手搓桩
const impl = (cmd: string) => (cmd === 'list_warranties' ? Promise.resolve([]) : Promise.reject(new Error('x')))
`,
      },
      ['list_warranties'],
    )
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain('list_warranties')
  })

  it('helpers/ 目录自身豁免（登记处即桩来源）', () => {
    const dir = makeFixture({
      'helpers/reference-stubs.ts': helperFixture().replace(
        'export const REFERENCE_DEFAULTS',
        "// 注释里演示 cmd === 'list_currencies' 不应误报\nexport const REFERENCE_DEFAULTS",
      ),
    })
    expect(run([dir]).status).toBe(0)
  })

  it('断言里的命令等值比较不误报（非桩接线）', () => {
    const dir = makeFixture({
      'Assert.test.ts': `const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_insurers')
expect(calls.length).toBeGreaterThan(0)
`,
    })
    expect(run([dir]).status).toBe(0)
  })

  it('助手缺失即 fail loud（退出码 1）', () => {
    const dir = mkdtempSync(join(tmpdir(), 'check-test-stubs-'))
    tempDirs.push(dir)
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain('reference-stubs.ts')
  })

  it('登记处提不出任何命令即 fail loud（清单漂移防护）', () => {
    const dir = mkdtempSync(join(tmpdir(), 'check-test-stubs-'))
    tempDirs.push(dir)
    mkdirSync(join(dir, 'helpers'), { recursive: true })
    writeFileSync(join(dir, 'helpers', 'reference-stubs.ts'), 'export const REFERENCE_DEFAULTS = {}\n')
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain('REFERENCE_DEFAULTS')
  })
})
