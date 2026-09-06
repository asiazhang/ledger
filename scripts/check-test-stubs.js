#!/usr/bin/env node
// 参考数据测试桩守门（issue #725）：前端测试文件不得再手搓参考数据 `list_*` 桩 if 链。
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
// （含 .test.ts 与共享桩模块），helpers/ 目录豁免（任意深度的同名目录，登记处即桩来源）；
// 本守门自身的包装测试 check-test-stubs.test.ts 豁免（其夹具文本合法包含违规形态）。
// 命中形态限桩接线：`if (cmd === '<命令>')`（if 链）、`cmd === '<命令>' ?`（三元）、
// `case '<命令>':`（switch）；断言里的命令等值比较（如 mock.calls.filter 箭头函数体）
// 非接线，不误报。已知文本不可达处（靠评审兜底）：桩实现形参改名（如 cmd → c）
// 即逃逸匹配；经变量间接分派、对象字面量覆写（overrides / invokeHandler defaults）
// 等合法形态同样不可达。
// 领域数据命令（list_transactions 等，不在登记处）的手搓桩不在守门范围。
//
// 用法：node scripts/check-test-stubs.js [testsDir]

import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join, relative, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const testsDir = resolve(process.argv[2] ?? join('src', '__tests__'))
const helperPath = join(testsDir, 'helpers', 'reference-stubs.ts')

function fail(message) {
  console.error(`✗ 参考数据测试桩守门：${message}`)
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

// —— 递归收集 .ts 文件（helpers/ 豁免） ——
function walk(dir) {
  const out = []
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, entry.name)
    if (entry.isDirectory()) {
      if (entry.name === 'helpers') continue
      out.push(...walk(p))
    } else if (entry.name.endsWith('.ts') && entry.name !== 'check-test-stubs.test.ts') {
      out.push(p)
    }
  }
  return out
}

function main() {
  const commands = extractCommands()
  if (commands.length === 0) {
    fail(`助手 ${helperPath} 的 REFERENCE_DEFAULTS 登记处提不出任何 list_* 命令（清单漂移？）`)
  }

  const violations = []
  for (const file of walk(testsDir)) {
    const rel = relative(testsDir, file)
    const lines = readFileSync(file, 'utf8').split('\n')
    lines.forEach((line, i) => {
      for (const cmd of commands) {
        const q = `['"\`]${cmd}['"\`]`
        const wiring = new RegExp(
          `if\\s*\\(\\s*cmd\\s*===\\s*${q}\\s*\\)` +
            `|cmd\\s*===\\s*${q}\\s*\\?` +
            `|case\\s+${q}\\s*:`,
        )
        if (wiring.test(line)) {
          violations.push(`  ${rel}:${i + 1}  手搓参考数据桩（${cmd}）——改用 helpers/reference-stubs.ts 的 stubReferenceInvoke`)
        }
      }
    })
  }

  if (violations.length > 0) {
    console.error(
      `✗ 参考数据测试桩守门：发现 ${violations.length} 处手搓桩（登记处命令：${commands.join(' ')}）\n` +
        violations.join('\n') +
        `\n参考数据桩单一来源：stubReferenceInvoke（${relative(process.cwd(), helperPath)}，issue #725）`,
    )
    process.exit(1)
  }

  console.log(`✅ 参考数据测试桩守门通过（登记处 ${commands.length} 条命令，testsDir=${relative(process.cwd(), testsDir) || '.'}）`)
}

// 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的扫描函数。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
