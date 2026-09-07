#!/usr/bin/env bun
// 前端测试桩守门（issue #725 + #726 + #750 + #822），四条规则：
//
// 规则 1（#725）：前端测试文件不得再手搓参考数据 `list_*` 桩接线。
//
// 背景：参考数据桩（list_currencies/list_accounts/list_categories/list_merchants/
// list_insurers）曾散落全仓 ~57 个测试文件，两分支并行各插一行后合并出同回调
// 重复桩——if 链先命中短路，后一条永远不生效，带数据桩被兜底空桩静默短路，
// 测试以「数据缺失」的间接方式失败，排查成本高。
// 治理：参考命令接线收敛到唯一接缝 wireInvokeSeam（helpers/invoke-mock.ts，
// 登记处在 helpers/reference-stubs.ts）；本脚本防回归。
//
// 命令清单单一来源：从助手的 `REFERENCE_DEFAULTS` 登记处文本提取命令名——
// 新增参考表只改助手，守门清单自动跟随，无双源漂移。登记处提不出任何命令
// 即红（清单漂移 fail loud）。
//
// 扫描边界：文本级扫描 `<testsDir>/**`（默认 src/__tests__）下全部 .ts 文件
// （含 .test.ts、helpers/ 测试助手与共享桩模块）。规则 1 与规则 3 豁免 helpers/（任意深度的同名
// 目录）——登记处即桩来源，接线合法；规则 2 不豁免 helpers/（#726 明确要求扫描测试
// helper，且 helper 内重复桩危害面更大）。本守门自身的包装测试
// check-test-stubs.test.ts 与接缝自测 invoke-seam.test.ts 文件级豁免（夹具文本合法
// 包含违规形态；接缝自测是接缝自身的符合性测试）。
// 命中形态限桩接线：`if (cmd === '<命令>')`（if 链）、`cmd === '<命令>' ?`（三元）、
// `case '<命令>':`（switch）；断言里的命令等值比较（如 mock.calls.filter 箭头函数体）
// 非接线，不误报。已知文本不可达处（靠评审兜底）：桩实现形参改名（如 cmd → c）
// 即逃逸匹配；经变量间接分派、对象字面量覆写（overrides / defaults 表）
// 等合法形态同样不可达。
// 领域数据命令（list_transactions 等，不在登记处）的手写分发桩由规则 3 纳管。
//
// 规则 2（#726）：同一 `mockImplementation` 回调体内同名命令的 `if (cmd === 'X')` 接线
// 不得重复（PR #721/#722/#723 合并曾产出 63 处同回调重复桩：if 链先命中短路，后一条
// 永不生效，带数据桩被兜底空桩静默短路，测试以「数据缺失」的间接方式失败）。规则 1
// 封禁参考命令接线后，领域命令（list_transactions 等）的手搓桩仍合法，本规则对其保留
// 兜底。实现：括号配对取回调体（词法跳过字符串字面量与注释，注释里不配对引号不再干
// 扰配对），嵌套 mockImplementation 体各自成独立单元（互不计数，DataLocationSettings
// 内层 cmd2 桩为合法先例）；接线匹配若落在注释/字符串里不计数，仅统计活代码（临时
// 注释掉接线不致误红）；对每个单元统计 `if (cmd === 'X')`（含 else if）出现次数，
// 同名 >1 即红，报文件、行号、命令名。
// 已知文本不可达处（靠评审兜底）：形参改名（cmd → cmd2/c）后逃逸匹配；三元/switch/
// 复合条件（&&/||）形态不在检测范围（参考命令的三元与 case 已由规则 1 覆盖）；
// mockImplementationOnce 是队列语义非整体替换，不纳入；正则字面量按普通字符扫描，
// 内含引号/括号时可致词法错位——失衡单元跳过不计（宁漏不误），或局部误判（漏报）；
// 恰好整体等于接线形态的字符串字面量理论上可误报（现实中未见）。同一文件两个独立
// 回调各桩同命令一次是整体替换语义，合法。
//
// 规则 3（#750，ADR-0085 决策 9）：迁移完成后禁回潮——测试辅助出口之外禁止
// 手写 invoke 分发桩与本地布线包装定义。
//   3a 手写分发桩：`.mockImplementation(`（非 Once）回调，首参名为 cmd，且回调体
//      活代码含命令分发特征（`if (cmd ===`、`cmd === '…' ?`、`switch (cmd)`）——
//      全量替换 + 命令分发即手写接缝，无论是否委托回接缝。特征串以 #750 收尾后
//      残余形态为准：钦定保留形态 = mockImplementationOnce 一次性委托（队列语义，
//      接缝文档典型用法）与无 cmd 形参的全量替换契约桩（裸值/在途用例），均不拦。
//   3b 本地布线包装：声明形出现 baseInvoke/stubInvoke/mockBaseCommands/invokeHandler
//      （迁移期已清零的历史布线包装名）即红；注释/字符串中提及不拦。
//   豁免范围与规则 1 同（helpers/ 目录自身、守门自测文件）另加接缝自测文件。
//   已知文本不可达处（靠评审兜底）：形参改名（cmd → c）与 async (cmd) 等前缀形态逃逸
//   3a；Once 无委托形态、非声明形的局部布线包装不可达。3b 按行首判定跳过注释行、
//   不做字符串掩码——字符串内恰好含完整声明形态文本理论上可误红（现实中未见）。
//
// 规则 4（#822）：测试文件内定义领域数据工厂即红——组件测试数据工厂唯一定义点
// 在共享工厂层出口（src/__tests__/factories.ts），测试文件本地定义即副本回潮。
//   名单（精确声明名）：makePlan / makeSubscriptionPlan / makeInstallmentPlan /
//   makeTransferPlan / makeOccurrence。交易侧 makeTxn / makeTransaction 待 #821
//   收敛落地后补入名单——本票与 #821 文件面不相交、互不阻塞，名单先行会让其
//   未收敛副本在守门直接变红。
//   白名单（相对 testsDir 路径）：factories.ts（唯一定义点）与
//   TransactionsView/common.ts（#821 交易薄壳一行包装，交易名补入名单时生效）。
//   双源代价（登记处）：名单与共享工厂层出口须人工同步——新增共享工厂必须同步
//   本名单，否则该厂的新副本不被拦截。
//   文本盲区（靠评审兜底）：改名逃逸（工厂改名或换名定义即逃逸名单）；仅识别
//   function/const/let/var 声明形，注释行整行跳过、字符串内无声明前缀不匹配，
//   与规则 3b 同款行首判定。规则 4 不豁免 helpers/——唯一定义点不在 helpers，
//   测试 helper 内本地定义名单工厂同样是回潮（豁免分工与规则 1/3 不同是有意为之）。
//
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-test-stubs.ts`。
// 用法：bun scripts/check-test-stubs.ts [testsDir]

import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join, relative, resolve, sep } from 'node:path'
import { pathToFileURL } from 'node:url'

const testsDir = resolve(process.argv[2] ?? join('src', '__tests__'))
const helperPath = join(testsDir, 'helpers', 'reference-stubs.ts')

function fail(message: string): never {
  console.error(`✗ 测试桩守门：${message}`)
  process.exit(1)
}

// —— 从助手登记处提取命令清单（单一来源） ——
function extractCommands(): string[] {
  if (!existsSync(helperPath)) {
    fail(`参考数据桩助手缺失：${helperPath}（issue #725 治理的桩单一来源）`)
  }
  const helperSource = readFileSync(helperPath, 'utf8')
  const registryMatch = helperSource.match(/REFERENCE_DEFAULTS[^=]*=\s*\{([\s\S]*?)\n\}/)
  if (!registryMatch) {
    fail(`助手 ${helperPath} 中找不到 REFERENCE_DEFAULTS 登记处，守门清单无从提取`)
  }
  return [...registryMatch[1].matchAll(/^\s*(list_[a-z_]+):/gm)].map((m) => m[1])
}

// —— 递归收集 .ts 文件（helpers/ 纳入扫描；守门自身包装测试与接缝自测豁免） ——
function walk(dir: string): string[] {
  const out: string[] = []
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, entry.name)
    if (entry.isDirectory()) {
      out.push(...walk(p))
    } else if (
      entry.name.endsWith('.ts') &&
      entry.name !== 'check-test-stubs.test.ts' &&
      entry.name !== 'invoke-seam.test.ts'
    ) {
      out.push(p)
    }
  }
  return out
}

// —— 规则 1：参考数据手搓桩接线（行级扫描；接线正则每个命令只编译一次） ——
function findHandWiredReferenceStubs(rel: string, source: string, commands: string[]): string[] {
  const wirings = commands.map((cmd) => {
    const q = `['"\`]${cmd}['"\`]`
    return {
      cmd,
      re: new RegExp(
        `if\\s*\\(\\s*cmd\\s*===\\s*${q}\\s*\\)` +
          `|cmd\\s*===\\s*${q}\\s*\\?` +
          `|case\\s+${q}\\s*:`,
      ),
    }
  })
  const hits: string[] = []
  source.split('\n').forEach((line, i) => {
    for (const { cmd, re } of wirings) {
      if (re.test(line)) {
        hits.push(`  ${rel}:${i + 1}  手搓参考数据桩（${cmd}）——改走唯一接缝 wireInvokeSeam（helpers/invoke-mock.ts）`)
      }
    }
  })
  return hits
}

// —— 规则 2：括号配对取出每个 mockImplementation 回调体范围（词法跳过字符串与注释，
//    并记录它们在单元内的范围供后续挖空） ——
interface CallbackUnit {
  start: number
  end: number
  masks: Array<[number, number]>
}

function extractCallbackUnits(source: string): CallbackUnit[] {
  const units: CallbackUnit[] = []
  const re = /\.mockImplementation\s*\(/g // 不匹配 mockImplementationOnce（队列语义，无静默短路）
  let m: RegExpExecArray | null
  while ((m = re.exec(source))) {
    const start = m.index + m[0].length
    let i = start
    let depth = 1
    const masks: Array<[number, number]> = []
    while (i < source.length && depth > 0) {
      const c = source[i]
      if (c === '/' && source[i + 1] === '/') {
        const s = i
        while (i < source.length && source[i] !== '\n') i++
        masks.push([s, i])
      } else if (c === '/' && source[i + 1] === '*') {
        const s = i
        i += 2
        while (i < source.length && !(source[i] === '*' && source[i + 1] === '/')) i++
        i = Math.min(i + 2, source.length)
        masks.push([s, i])
      } else if (c === '"' || c === "'" || c === '`') {
        const s = i
        const quote = c
        i++
        while (i < source.length && source[i] !== quote) {
          if (source[i] === '\\') i++
          i++
        }
        i = Math.min(i + 1, source.length)
        masks.push([s, i])
      } else {
        if (c === '(') depth++
        else if (c === ')') depth--
        i++
      }
    }
    if (depth !== 0) continue // 配对失衡（字面量错位、非完整片段）：跳过该单元，宁漏不误
    units.push({ start, end: i - 1, masks })
  }
  return units
}

function lineOf(source: string, offset: number): number {
  let line = 1
  for (let i = 0; i < offset; i++) if (source[i] === '\n') line++
  return line
}

// —— 规则 2：同回调内同名命令 if 接线去重 ——
// 仅统计活代码：接线匹配若落在注释/字符串内（与某个注释/字符串范围重叠且伸出其外），
// 不计数；匹配自身携带的命令字符串完全包含于匹配内，不影响判定。
function findDuplicateWiring(rel: string, source: string, units: CallbackUnit[]): string[] {
  const hits: string[] = []
  const IF_WIRING = /\bif\s*\(\s*cmd\s*===\s*(['"`])([^'"`]+)\1\s*\)/g
  const isLive = (at: number, len: number, masks: Array<[number, number]>): boolean =>
    !masks.some(([ms, me]) => ms < at + len && me > at && (ms < at || me > at + len))
  for (const unit of units) {
    const localMasks = unit.masks.map(
      ([s, e]) => [s - unit.start, Math.min(e, unit.end) - unit.start] as [number, number],
    )
    let text = source.slice(unit.start, unit.end)
    for (const nested of units) {
      if (nested === unit) continue
      if (nested.start >= unit.start && nested.end <= unit.end) {
        const s = nested.start - unit.start
        const e = nested.end - unit.start
        text = text.slice(0, s) + text.slice(s, e).replace(/[^\n]/g, ' ') + text.slice(e) // 同长挖空，稳住行号
      }
    }
    const byCmd = new Map<string, number[]>()
    let m: RegExpExecArray | null
    IF_WIRING.lastIndex = 0
    while ((m = IF_WIRING.exec(text))) {
      if (!isLive(m.index, m[0].length, localMasks)) continue
      if (!byCmd.has(m[2])) byCmd.set(m[2], [])
      byCmd.get(m[2])!.push(lineOf(source, unit.start + m.index))
    }
    for (const [cmd, lines] of byCmd) {
      if (lines.length > 1) {
        hits.push(`  ${rel}:${lines[0]}  同回调重复桩（${cmd} ×${lines.length}，行 ${lines.join('、')}）——if 链先命中短路，后一条永不生效`)
      }
    }
  }
  return hits
}

// —— 规则 3a：手写 invoke 分发桩（全量替换 mockImplementation 回调 + cmd 首参 +
//    活代码命令分发特征）。Once 一次性委托（队列语义）与无 cmd 形参的契约桩不拦。
const DISPATCH_FEATURE = new RegExp(
  [
    '\\bif\\s*\\(\\s*cmd\\s*===', // if (cmd === '…')
    '\\bcmd\\s*===\\s*[\'"`][^\'"`\\n]*[\'"`]\\s*\\?', // cmd === '…' ?
    '\\bswitch\\s*\\(\\s*cmd\\s*\\)', // switch (cmd)
  ].join('|'),
  'g',
)

function findHandWrittenDispatchStub(rel: string, source: string, units: CallbackUnit[]): string[] {
  const hits: string[] = []
  for (const unit of units) {
    const raw = source.slice(unit.start, unit.end)
    // 首参名限定 cmd（形参改名即逃逸匹配，已知文本不可达处）；cmd2 等不误伤
    if (!/^\s*\(?\s*cmd\b/.test(raw)) continue
    // 挖空嵌套回调单元与字符串/注释（与规则 2 同款词法掩码），只看本单元活代码
    let text = raw
    for (const nested of units) {
      if (nested === unit) continue
      if (nested.start >= unit.start && nested.end <= unit.end) {
        const st = nested.start - unit.start
        const en = nested.end - unit.start
        text = text.slice(0, st) + text.slice(st, en).replace(/[^\n]/g, ' ') + text.slice(en)
      }
    }
    const masks = unit.masks.map(
      ([ms, me]) => [ms - unit.start, Math.min(me, unit.end) - unit.start] as [number, number],
    )
    DISPATCH_FEATURE.lastIndex = 0
    let m: RegExpExecArray | null
    let dispatched = false
    while ((m = DISPATCH_FEATURE.exec(text))) {
      const at = m.index
      const len = m[0].length
      // 存在性检测：匹配整体落在字符串/注释掩码内才算死（三元特征必然内含命令名
      // 字符串字面量，部分重叠不算死——与规则 2 的计数口径有意不同）
      const dead = masks.some(([ms, me]) => ms <= at && at + len <= me)
      if (!dead) {
        dispatched = true
        break
      }
    }
    if (dispatched) {
      hits.push(
        `  ${rel}:${lineOf(source, unit.start)}  手写 invoke 分发桩（mockImplementation 全量替换 + cmd 分发）——改走唯一接缝 wireInvokeSeam 两表布线，一次性覆盖用 mockImplementationOnce 委托`,
      )
    }
  }
  return hits
}

// —— 规则 3b：本地布线包装定义（迁移期已清零的历史布线包装名，声明形出现即红；
//    注释/字符串中提及不拦——注释行整行跳过，字符串内无声明关键字前缀不匹配） ——
const WRAPPER_DECL =
  /\b(?:function\s+|(?:const|let|var)\s+)(baseInvoke|stubInvoke|mockBaseCommands|invokeHandler)\b/g

function findLocalWiringWrapper(rel: string, source: string): string[] {
  const hits: string[] = []
  source.split('\n').forEach((line, i) => {
    const t = line.trim()
    if (t.startsWith('//') || t.startsWith('*') || t.startsWith('/*')) return
    WRAPPER_DECL.lastIndex = 0
    let m: RegExpExecArray | null
    while ((m = WRAPPER_DECL.exec(line))) {
      hits.push(`  ${rel}:${i + 1}  本地布线包装定义（${m[1]}）——布线一律走唯一接缝 wireInvokeSeam`)
    }
  })
  return hits
}

// —— 规则 4：领域数据工厂本地定义（声明形出现即红；注释行整行跳过，字符串内
//    无声明关键字前缀不匹配——与规则 3b 同款行首判定） ——
// 名单与共享工厂层出口人工同步（双源代价，见头注释）：新增共享工厂必须同步此清单；
// 交易侧 makeTxn/makeTransaction 待 #821 收敛落地后补入。
const FACTORY_NAMES = [
  'makePlan',
  'makeSubscriptionPlan',
  'makeInstallmentPlan',
  'makeTransferPlan',
  'makeOccurrence',
]
const FACTORY_DECL = new RegExp(
  `\\b(?:function\\s+|(?:const|let|var)\\s+)(${FACTORY_NAMES.join('|')})\\b`,
  'g',
)
// 白名单按相对 testsDir 的 posix 路径登记：唯一定义点 + 交易薄壳一行包装（#821）
const FACTORY_WHITELIST = new Set(['factories.ts', join('TransactionsView', 'common.ts')])

function findFactoryDefinition(rel: string, source: string): string[] {
  const hits: string[] = []
  source.split('\n').forEach((line, i) => {
    const t = line.trim()
    if (t.startsWith('//') || t.startsWith('*') || t.startsWith('/*')) return
    FACTORY_DECL.lastIndex = 0
    let m: RegExpExecArray | null
    while ((m = FACTORY_DECL.exec(line))) {
      hits.push(
        `  ${rel}:${i + 1}  领域数据工厂本地定义（${m[1]}）——组件测试数据工厂唯一定义点在共享工厂层，消费共享出口而非本地定义`,
      )
    }
  })
  return hits
}

function main(): void {
  const commands = extractCommands()
  if (commands.length === 0) {
    fail(`助手 ${helperPath} 的 REFERENCE_DEFAULTS 登记处提不出任何 list_* 命令（清单漂移？）`)
  }

  let handWired = 0
  let duplicated = 0
  let dispatchStubs = 0
  let wiringWrappers = 0
  let factoryDefs = 0
  const violations: string[] = []
  for (const file of walk(testsDir)) {
    const rel = relative(testsDir, file)
    const source = readFileSync(file, 'utf8')
    const inHelpers = rel.split(sep).includes('helpers')
    const units = extractCallbackUnits(source)
    const rule1 = inHelpers ? [] : findHandWiredReferenceStubs(rel, source, commands)
    const rule2 = findDuplicateWiring(rel, source, units)
    const rule3a = inHelpers ? [] : findHandWrittenDispatchStub(rel, source, units)
    const rule3b = inHelpers ? [] : findLocalWiringWrapper(rel, source)
    const rule4 = FACTORY_WHITELIST.has(rel.split(sep).join('/'))
      ? []
      : findFactoryDefinition(rel, source)
    handWired += rule1.length
    duplicated += rule2.length
    dispatchStubs += rule3a.length
    wiringWrappers += rule3b.length
    factoryDefs += rule4.length
    violations.push(...rule1, ...rule2, ...rule3a, ...rule3b, ...rule4)
  }

  if (violations.length > 0) {
    console.error(
      `✗ 测试桩守门：发现 ${handWired} 处手搓参考数据桩、${duplicated} 处同回调重复桩、${dispatchStubs} 处手写 invoke 分发桩、${wiringWrappers} 处本地布线包装、${factoryDefs} 处领域数据工厂本地定义（登记处命令：${commands.join(' ')}）\n` +
        violations.join('\n') +
        `\ninvoke 布线唯一接缝：wireInvokeSeam（${relative(process.cwd(), join(testsDir, 'helpers', 'invoke-mock.ts'))}，issue #746/#750，ADR-0085）`,
    )
    process.exit(1)
  }

  console.log(`✅ 测试桩守门通过（登记处 ${commands.length} 条命令；同回调重复 0、手写分发桩 0、本地布线包装 0、领域数据工厂本地定义 0，testsDir=${relative(process.cwd(), testsDir) || '.'}）`)
}

// 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的扫描函数。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
