#!/usr/bin/env node
// 前端测试桩守门（issue #725 + #726），两条规则：
//
// 规则 1（#725）：前端测试文件不得再手搓参考数据 `list_*` 桩接线。
//
// 背景：参考数据桩（list_currencies/list_accounts/list_categories/list_merchants/
// list_insurers）曾散落全仓 ~57 个测试文件，两分支并行各插一行后合并出同回调
// 重复桩——if 链先命中短路，后一条永远不生效，带数据桩被兜底空桩静默短路，
// 测试以「数据缺失」的间接方式失败，排查成本高。
// 治理：桩来源收敛到 src/__tests__/helpers/reference-stubs.ts（stubReferenceInvoke，
// 深模块单一来源）；本脚本防回归。
//
// 命令清单单一来源：从助手的 `REFERENCE_DEFAULTS` 登记处文本提取命令名——
// 新增参考表只改助手，守门清单自动跟随，无双源漂移。登记处提不出任何命令
// 即红（清单漂移 fail loud）。
//
// 扫描边界：文本级扫描 `<testsDir>/**`（默认 src/__tests__）下全部 .ts 文件
// （含 .test.ts、helpers/ 测试助手与共享桩模块）。规则 1 豁免 helpers/（任意深度的同名
// 目录）——登记处即桩来源，接线合法；规则 2 不豁免 helpers/（#726 明确要求扫描测试
// helper，且 helper 内重复桩危害面更大）。本守门自身的包装测试
// check-test-stubs.test.ts 豁免（其夹具文本合法包含违规形态）。
// 命中形态限桩接线：`if (cmd === '<命令>')`（if 链）、`cmd === '<命令>' ?`（三元）、
// `case '<命令>':`（switch）；断言里的命令等值比较（如 mock.calls.filter 箭头函数体）
// 非接线，不误报。已知文本不可达处（靠评审兜底）：桩实现形参改名（如 cmd → c）
// 即逃逸匹配；经变量间接分派、对象字面量覆写（overrides / invokeHandler defaults）
// 等合法形态同样不可达。
// 领域数据命令（list_transactions 等，不在登记处）的手搓桩不在守门范围。
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
// 用法：node scripts/check-test-stubs.js [testsDir]

import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join, relative, resolve, sep } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const testsDir = resolve(process.argv[2] ?? join('src', '__tests__'))
const helperPath = join(testsDir, 'helpers', 'reference-stubs.ts')

function fail(message) {
  console.error(`✗ 测试桩守门：${message}`)
  process.exit(1)
}

// —— 从助手登记处提取命令清单（单一来源） ——
function extractCommands() {
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

// —— 递归收集 .ts 文件（helpers/ 纳入扫描；守门自身包装测试豁免） ——
function walk(dir) {
  const out = []
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, entry.name)
    if (entry.isDirectory()) {
      out.push(...walk(p))
    } else if (entry.name.endsWith('.ts') && entry.name !== 'check-test-stubs.test.ts') {
      out.push(p)
    }
  }
  return out
}

// —— 规则 1：参考数据手搓桩接线（行级扫描；接线正则每个命令只编译一次） ——
function findHandWiredReferenceStubs(rel, source, commands) {
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
  const hits = []
  source.split('\n').forEach((line, i) => {
    for (const { cmd, re } of wirings) {
      if (re.test(line)) {
        hits.push(`  ${rel}:${i + 1}  手搓参考数据桩（${cmd}）——改用 helpers/reference-stubs.ts 的 stubReferenceInvoke`)
      }
    }
  })
  return hits
}

// —— 规则 2：括号配对取出每个 mockImplementation 回调体范围（词法跳过字符串与注释，
//    并记录它们在单元内的范围供后续挖空） ——
function extractCallbackUnits(source) {
  const units = []
  const re = /\.mockImplementation\s*\(/g // 不匹配 mockImplementationOnce（队列语义，无静默短路）
  let m
  while ((m = re.exec(source))) {
    const start = m.index + m[0].length
    let i = start
    let depth = 1
    const masks = []
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

function lineOf(source, offset) {
  let line = 1
  for (let i = 0; i < offset; i++) if (source[i] === '\n') line++
  return line
}

// —— 规则 2：同回调内同名命令 if 接线去重 ——
// 仅统计活代码：接线匹配若落在注释/字符串内（与某个注释/字符串范围重叠且伸出其外），
// 不计数；匹配自身携带的命令字符串完全包含于匹配内，不影响判定。
function findDuplicateWiring(rel, source, units) {
  const hits = []
  const IF_WIRING = /\bif\s*\(\s*cmd\s*===\s*(['"`])([^'"`]+)\1\s*\)/g
  const isLive = (at, len, masks) =>
    !masks.some(([ms, me]) => ms < at + len && me > at && (ms < at || me > at + len))
  for (const unit of units) {
    const localMasks = unit.masks.map(([s, e]) => [s - unit.start, Math.min(e, unit.end) - unit.start])
    let text = source.slice(unit.start, unit.end)
    for (const nested of units) {
      if (nested === unit) continue
      if (nested.start >= unit.start && nested.end <= unit.end) {
        const s = nested.start - unit.start
        const e = nested.end - unit.start
        text = text.slice(0, s) + text.slice(s, e).replace(/[^\n]/g, ' ') + text.slice(e) // 同长挖空，稳住行号
      }
    }
    const byCmd = new Map()
    let m
    IF_WIRING.lastIndex = 0
    while ((m = IF_WIRING.exec(text))) {
      if (!isLive(m.index, m[0].length, localMasks)) continue
      if (!byCmd.has(m[2])) byCmd.set(m[2], [])
      byCmd.get(m[2]).push(lineOf(source, unit.start + m.index))
    }
    for (const [cmd, lines] of byCmd) {
      if (lines.length > 1) {
        hits.push(`  ${rel}:${lines[0]}  同回调重复桩（${cmd} ×${lines.length}，行 ${lines.join('、')}）——if 链先命中短路，后一条永不生效`)
      }
    }
  }
  return hits
}

function main() {
  const commands = extractCommands()
  if (commands.length === 0) {
    fail(`助手 ${helperPath} 的 REFERENCE_DEFAULTS 登记处提不出任何 list_* 命令（清单漂移？）`)
  }

  let handWired = 0
  let duplicated = 0
  const violations = []
  for (const file of walk(testsDir)) {
    const rel = relative(testsDir, file)
    const source = readFileSync(file, 'utf8')
    const inHelpers = rel.split(sep).includes('helpers')
    const rule1 = inHelpers ? [] : findHandWiredReferenceStubs(rel, source, commands)
    const rule2 = findDuplicateWiring(rel, source, extractCallbackUnits(source))
    handWired += rule1.length
    duplicated += rule2.length
    violations.push(...rule1, ...rule2)
  }

  if (violations.length > 0) {
    console.error(
      `✗ 测试桩守门：发现 ${handWired} 处手搓参考数据桩、${duplicated} 处同回调重复桩（登记处命令：${commands.join(' ')}）\n` +
        violations.join('\n') +
        `\n参考数据桩单一来源：stubReferenceInvoke（${relative(process.cwd(), helperPath)}，issue #725）`,
    )
    process.exit(1)
  }

  console.log(`✅ 测试桩守门通过（登记处 ${commands.length} 条命令；同回调重复 0，testsDir=${relative(process.cwd(), testsDir) || '.'}）`)
}

// 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的扫描函数。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
