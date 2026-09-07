import { afterAll, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { REFERENCE_DEFAULTS } from './helpers/reference-stubs'

// 被测对象是仓库工具脚本 scripts/check-test-stubs.ts（前端测试桩守门，issue #725/#726/#822）。
// 脚本以 Bun 运行时执行（ADR-0083）：spawnSync('bun') 与门槛调用同款，测的就是门槛路径。
// 按测试决策只测外部可观察结果——进程退出码与输出，不测内部函数；
// 通过位置参数把扫描目标指向临时夹具目录。
// 夹具命令清单自助手导出的 REFERENCE_DEFAULTS 派生（单一事实源，无双源漂移）；
// （vitest 转换后 import.meta.url 非 file: scheme，取进程 cwd = 仓库根定位脚本）
const script = join(process.cwd(), 'scripts', 'check-test-stubs.ts')

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

const CLEAN_TEST = `import { mockInvoke, wireInvokeSeam } from './helpers/invoke-mock'
// 参考命令由接缝内建兜底；领域命令走 overrides 表或一次性委托（钦定形态）
wireInvokeSeam({ overrides: { list_transactions: [] } })
const base = wireInvokeSeam()
mockInvoke.mockImplementationOnce((cmd: string, args?: Record<string, unknown>) =>
  cmd === 'create_transaction' ? Promise.resolve('new-id') : base(cmd, args))
`

describe('check-test-stubs', () => {
  it('全仓测试走唯一接缝与钦定一次性委托时通过（退出码 0）', () => {
    const dir = makeFixture({ 'SomeView.test.ts': CLEAN_TEST })
    const r = run([dir])
    expect(r.status).toBe(0)
    expect(r.output).toContain('测试桩守门')
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

  it('领域数据命令（不在登记处）的手写分发桩同样红（规则 3）', () => {
    const dir = makeFixture({
      'Domain.test.ts': `mockInvoke.mockImplementation((cmd: string) => {
  if (cmd === 'list_policies') return Promise.resolve([])
  return Promise.reject(new Error('unexpected invoke'))
})`,
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain('Domain.test.ts')
    expect(r.output).toContain('手写 invoke 分发桩')
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

describe('check-test-stubs 同回调重复接线检测（#726）', () => {
  it('同回调重复 if 同名命令即红：报文件、行号、命令名（领域命令同样纳管）', () => {
    const dir = makeFixture({
      'Dup.test.ts': `mockInvoke.mockImplementation((cmd: string) => {
  if (cmd === 'list_transactions') return Promise.resolve([a])
  if (cmd === 'get_settings') return Promise.resolve(s)
  if (cmd === 'list_transactions') return Promise.resolve([b])
  return Promise.reject(new Error('unexpected invoke'))
})`,
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain('Dup.test.ts')
    expect(r.output).toContain('同回调重复桩')
    expect(r.output).toContain('list_transactions')
    expect(r.output).toContain('行 2、4')
  })

  it('同回调不同命令各桩一次不触发重复规则（规则 3 另行拦截手写分发桩）', () => {
    const dir = makeFixture({
      'Multi.test.ts': `mockInvoke.mockImplementation((cmd: string) => {
  if (cmd === 'list_transactions') return Promise.resolve([])
  if (cmd === 'get_settings') return Promise.resolve({})
  return Promise.reject(new Error('unexpected invoke'))
})`,
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).not.toContain('同回调重复桩（')
    expect(r.output).toContain('手写 invoke 分发桩')
  })

  it('嵌套回调各自成单元不误报重复（外层分发桩由规则 3 拦截）', () => {
    const dir = makeFixture({
      'Nested.test.ts': `mockInvoke.mockImplementation((cmd: string) => {
  if (cmd === 'submit_data_location_change') {
    mockInvoke.mockImplementation((cmd2: string) => {
      if (cmd2 === 'get_data_location_info') return Promise.resolve(info)
      return Promise.reject(new Error('unexpected invoke'))
    })
    return Promise.resolve(committed)
  }
  if (cmd === 'get_data_location_info') return Promise.resolve(baseInfo)
  return Promise.reject(new Error('unexpected invoke'))
})`,
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).not.toContain('同回调重复桩（')
    expect(r.output).toContain('手写 invoke 分发桩')
  })

  it('嵌套内外层同名形参 cmd 各桩一次同命令，不误报重复（规则 3 各自分发桩另计）', () => {
    const dir = makeFixture({
      'NestedSameParam.test.ts': `mockInvoke.mockImplementation((cmd: string) => {
  if (cmd === 'get_settings') return Promise.resolve(s)
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'get_settings') return Promise.resolve(s2)
    return Promise.reject(new Error('unexpected invoke'))
  })
  return Promise.reject(new Error('unreachable'))
})`,
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).not.toContain('同回调重复桩（')
  })

  it('else-if 形态的重复同样识别', () => {
    const dir = makeFixture({
      'ElseIf.test.ts': `mockInvoke.mockImplementation((cmd: string) => {
  if (cmd === 'list_transactions') return Promise.resolve([a])
  else if (cmd === 'list_transactions') return Promise.resolve([b])
  return Promise.reject(new Error('unexpected invoke'))
})`,
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain('ElseIf.test.ts')
    expect(r.output).toContain('行 2、3')
  })

  it('两个独立 mockImplementation 各桩同命令一次：不触发重复规则（分发桩由规则 3 拦截）', () => {
    const dir = makeFixture({
      'Restub.test.ts': `mockInvoke.mockImplementation((cmd: string) => {
  if (cmd === 'get_settings') return Promise.resolve(s)
  return Promise.reject(new Error('unexpected invoke'))
})
mockInvoke.mockImplementation((cmd: string) => {
  if (cmd === 'get_settings') return Promise.resolve(s2)
  return Promise.reject(new Error('unexpected invoke'))
})`,
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).not.toContain('同回调重复桩（')
  })

  it('helpers/ 内的同回调重复桩同样拦截（规则 2 扫描测试 helper；规则 3 豁免 helper）', () => {
    const dir = makeFixture({
      'helpers/domain-stubs.ts': `export function stubDomain(mockInvoke: { mockImplementation: (f: unknown) => void }) {
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_transactions') return Promise.resolve([])
    if (cmd === 'list_transactions') return Promise.resolve([])
    return Promise.reject(new Error('unexpected invoke'))
  })
}`, 
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain(join('helpers', 'domain-stubs.ts'))
    expect(r.output).toContain('同回调重复桩')
    expect(r.output).not.toContain('手写 invoke 分发桩（')
  })

  it('回调体内注释含不配对引号/括号不影响括号配对（词法跳过注释，活分发仍被规则 3 拦截）', () => {
    const dir = makeFixture({
      'Comments.test.ts': `mockInvoke.mockImplementation((cmd: string) => {
  // don't stub twice (it's covered by the guard's own fixture)
  /* 块注释带 ( 不配对括号与 ' 引号 */
  if (cmd === 'get_settings') return Promise.resolve(s)
  return Promise.reject(new Error('unexpected invoke'))
})
mockInvoke.mockImplementation((cmd: string) => {
  if (cmd === 'get_settings') return Promise.resolve(s2)
  return Promise.reject(new Error('unexpected invoke'))
})`,
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain('手写 invoke 分发桩')
    expect(r.output).not.toContain('同回调重复桩（')
  })

  it('字符串字面量内的接线形文本不计数（重复规则不误报；活分发由规则 3 拦截）', () => {
    const dir = makeFixture({
      'StringShape.test.ts': `mockInvoke.mockImplementation((cmd: string) => {
  const hint = "if (cmd === 'get_settings') is a wiring shape, not wiring"
  if (cmd === 'get_settings') return Promise.resolve(s)
  return Promise.reject(new Error('unexpected invoke'))
})`,
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).not.toContain('同回调重复桩（')
    expect(r.output).toContain('手写 invoke 分发桩')
  })

  it('注释掉的接线不计数（临时注释掉不致守门误红；活分发由规则 3 拦截）', () => {
    const dir = makeFixture({
      'DeadWiring.test.ts': `mockInvoke.mockImplementation((cmd: string) => {
  // if (cmd === 'get_settings') return Promise.resolve(dead)
  if (cmd === 'get_settings') return Promise.resolve(s)
  return Promise.reject(new Error('unexpected invoke'))
})`,
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).not.toContain('同回调重复桩（')
    expect(r.output).toContain('手写 invoke 分发桩')
  })
})

describe('check-test-stubs 手写分发桩与本地布线包装检测（#750 规则 3）', () => {
  it('三元分发形态的全量替换桩同样红', () => {
    const dir = makeFixture({
      'Ternary3.test.ts': `mockInvoke.mockImplementation((cmd: string, args?: Record<string, unknown>) =>
  cmd === 'create_transaction' ? Promise.resolve('new-id') : undefined)`,
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain('Ternary3.test.ts')
    expect(r.output).toContain('手写 invoke 分发桩')
  })

  it('case 分发形态的全量替换桩同样红', () => {
    const dir = makeFixture({
      'Switch3.test.ts': `mockInvoke.mockImplementation((cmd: string) => {
  switch (cmd) {
    case 'list_transactions':
      return Promise.resolve([])
    default:
      return Promise.reject(new Error('x'))
  }
})`,
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain('Switch3.test.ts')
    expect(r.output).toContain('手写 invoke 分发桩')
  })

  it('mockImplementationOnce 一次性委托不触发（接缝文档钦定先例形态）', () => {
    const dir = makeFixture({
      'Once.test.ts': `const base = wireInvokeSeam()
mockInvoke.mockImplementationOnce((cmd: string, args?: Record<string, unknown>) =>
  cmd === 'create_transaction' ? Promise.resolve('new-id') : base(cmd, args))`,
    })
    expect(run([dir]).status).toBe(0)
  })

  it('无 cmd 形参的全量替换不触发（裸值/在途契约用例先例）', () => {
    const dir = makeFixture({
      'NoCmdParam.test.ts': `mockInvoke.mockImplementation((() => []) as unknown as AppInvokeHandler)
mockInvoke.mockImplementation(() => new Promise(() => {}))`,
    })
    expect(run([dir]).status).toBe(0)
  })

  it('分发特征仅存在于字符串或注释中不触发（词法掩码）', () => {
    const dir = makeFixture({
      'Masked.test.ts': `mockInvoke.mockImplementation((cmd: string) => {
  const hint = "if (cmd === 'get_settings') is text, not wiring"
  // cmd === 'get_settings' ? never() : never()
  return Promise.resolve({})
})`,
    })
    expect(run([dir]).status).toBe(0)
  })

  it('形参改名（cmd → c）逃逸匹配（已知文本不可达，靠评审兜底）', () => {
    const dir = makeFixture({
      'Renamed.test.ts': `mockInvoke.mockImplementation((c: string) => {
  if (c === 'list_transactions') return Promise.resolve([])
  return Promise.reject(new Error('unexpected invoke'))
})`,
    })
    expect(run([dir]).status).toBe(0)
  })

  it('本地布线包装声明即红（baseInvoke/stubInvoke/mockBaseCommands/invokeHandler）', () => {
    const dir = makeFixture({
      'Wrapper.test.ts': `function baseInvoke(defaults: Record<string, unknown>) {}
const stubInvoke = (table: Record<string, unknown>) => {}
let mockBaseCommands: unknown
var invokeHandler = () => {}`,
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    for (const name of ['baseInvoke', 'stubInvoke', 'mockBaseCommands', 'invokeHandler']) {
      expect(r.output).toContain(name)
    }
    expect(r.output).toContain('本地布线包装')
  })

  it('注释或字符串中提及布线包装名不触发（声明形才拦截）', () => {
    const dir = makeFixture({
      'Mention.test.ts': `// 旧形态 function baseInvoke(...) 已删除，历史见 issue #750
const note = 'stubInvoke was here'
export const doc = { name: 'mockBaseCommands' }
export const legacy = { fn: 'invokeHandler' }`,
    })
    expect(run([dir]).status).toBe(0)
  })

  it('接缝自测文件（invoke-seam.test.ts）文件级豁免', () => {
    const dir = makeFixture({
      'invoke-seam.test.ts': `mockInvoke.mockImplementation((cmd: string) => {
  if (cmd === 'list_transactions') return Promise.resolve([])
  return Promise.reject(new Error('unexpected invoke'))
})`,
    })
    expect(run([dir]).status).toBe(0)
  })

  it('helpers/ 目录自身豁免（规则 3 豁免范围与规则 1 同）', () => {
    const dir = makeFixture({
      'helpers/seam-like.ts': `export function wireThing() {
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_transactions') return Promise.resolve([])
    return Promise.reject(new Error('unexpected invoke'))
  })
}`, 
    })
    expect(run([dir]).status).toBe(0)
  })
})

describe('check-test-stubs 领域数据工厂本地定义检测（#822 规则 4）', () => {
  it('测试文件 function 声明名单内工厂即红：报文件与工厂名', () => {
    const dir = makeFixture({
      'SomeView.test.ts': `import { makeSubscriptionPlan } from './factories'
function makePlan(partial: { id: string }) {
  return { id: partial.id, amount_cents: 1500 }
}
const plan = makePlan({ id: 'p1' })
`,
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain('SomeView.test.ts')
    expect(r.output).toContain('makePlan')
    expect(r.output).toContain('领域数据工厂本地定义')
  })

  it('const/let 声明形（含别名绑定）同样拦：别名禁用', () => {
    const dir = makeFixture({
      'Aliased.test.ts': `import { makeSubscriptionPlan as shared } from './factories'
const makeOccurrence = shared  // 本地别名绑定也是定义，别名禁用
export { makeOccurrence }
`,
      'LetForm.test.ts': `let makeTransferPlan: unknown
export { makeTransferPlan }
`,
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    expect(r.output).toContain('makeOccurrence')
    expect(r.output).toContain('makeTransferPlan')
  })

  it('名单逐一纳管：三形态厂与期次厂均拦', () => {
    const dir = makeFixture({
      'A.test.ts': 'function makeSubscriptionPlan(p: unknown) { return p }\n',
      'B.test.ts': 'function makeInstallmentPlan(p: unknown) { return p }\n',
      'C.test.ts': 'const makeTransferPlan = (p: unknown) => p\n',
      'D.test.ts': 'export function makeOccurrence(p: unknown) { return p }\n',
    })
    const r = run([dir])
    expect(r.status).toBe(1)
    for (const name of [
      'makeSubscriptionPlan',
      'makeInstallmentPlan',
      'makeTransferPlan',
      'makeOccurrence',
    ]) {
      expect(r.output).toContain(name)
    }
  })

  it('共享工厂层出口 factories.ts 白名单不拦（唯一定义点）', () => {
    const dir = makeFixture({
      'factories.ts': `export function makePlan(partial: { id: string }) {
  return { id: partial.id }
}
export const makeOccurrence = (partial: { id: string }) => ({ id: partial.id })
`,
    })
    expect(run([dir]).status).toBe(0)
  })

  it('目录薄壳包装白名单（TransactionsView/common.ts）不拦', () => {
    const dir = makeFixture({
      'TransactionsView/common.ts': `export function makePlan(partial: { id: string }) {
  return { id: partial.id }
}
`,
    })
    expect(run([dir]).status).toBe(0)
  })

  it('消费共享出口（import 绑定与调用）不拦：只拦定义', () => {
    const dir = makeFixture({
      'Consumer.test.ts': `import { makePlan, makeOccurrence } from './factories'
const plan = makePlan({ id: 'p1' })
const occ = makeOccurrence({ id: 'o1' })
`,
    })
    expect(run([dir]).status).toBe(0)
  })

  it('同名前缀的无关函数不误报（边界判定 \b）', () => {
    const dir = makeFixture({
      'Prefix.test.ts': `function makePlanner(p: unknown) { return p }
const makePlans = (p: unknown) => p
`,
    })
    expect(run([dir]).status).toBe(0)
  })

  it('注释/字符串中提及工厂名不拦（声明形才拦，与规则 3b 同款行首判定）', () => {
    const dir = makeFixture({
      'Mention4.test.ts': `// 历史副本 function makePlan(...) 已删除，见 issue #822
/* makeOccurrence 曾经在此定义 */
const note = '字符串里的 makeTransferPlan 提及不拦'
const legacy = { factory: 'makeInstallmentPlan' }
`,
    })
    expect(run([dir]).status).toBe(0)
  })
})
