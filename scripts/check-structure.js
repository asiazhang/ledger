#!/usr/bin/env node
// 结构守门（issue #396 / ADR-0056）：白名单式分层依赖检查。
// 分层规则：壳 → 域 → 基础设施，域永不依赖壳。白名单 = 已归位域目录 + 全部
// 基础设施（「已验证对壳层零依赖」固化为规格）；白名单内出现对壳层
// （src-tauri/src/commands/）的模块路径依赖即红——每归位一域追加一行白名单。
// 测试豁免（ADR-0056 决策 5）：外挂测试模块/目录（tests.rs 文件、tests/ 目录）
// 不参与守门——BDD/单元 fixture 合法引用壳层入口，不制造虚假违规；
// 内联 #[cfg(test)] 模块不豁免。白名单路径缺失或条目内扫不到非测试 Rust 文件
// 即红（清单漂移 fail loud）。
// 扫描边界：文本级扫描，注释与字符串/char 字面量掩码后匹配 `commands::`
// 路径引用与 `commands as` 别名引入；经别名改名的间接引用文本不可达，
// 靠评审兜底。
// 默认校验本仓库；测试可传位置参数指向夹具：node scripts/check-structure.js [src-dir]
// 挂载于 scripts/check.sh 质量门槛序列与 CI（build.yml frontend job），
// 与命令注册一致性检查并列。

import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'

/**
 * 守门白名单（ADR-0056 决策 4）：路径相对 src-tauri/src。
 * 首批 = 已归位域目录 + 全部基础设施；每迁一域在此追加一行。
 */
export const WHITELIST = [
  { path: 'transaction', layer: '域目录', note: '核心交易域' },
  { path: 'scheduled_transactions', layer: '域目录', note: '定时计划域' },
  { path: 'item', layer: '域目录', note: '物品域（#397 阶段 1 归位，主体自 commands/item 随迁）' },
  { path: 'policy', layer: '域目录', note: '保单域（#398 阶段 2 归位）' },
  { path: 'budget', layer: '域目录', note: '预算域（#399 阶段 3 归位）' },
  { path: 'merchants', layer: '域目录', note: '商户域（#400 阶段 4 归位）' },
  { path: 'investment', layer: '域目录', note: '投资域（#401 阶段 5 归位，主体自 commands/investment 随迁；价格写入单点自 sync/persist 迁入）' },
  { path: 'accounts', layer: '域目录', note: '账户域（#404 参考数据域归位，主体自 commands/accounts 随迁）' },
  { path: 'categories', layer: '域目录', note: '分类域（#404 参考数据域归位，主体自 commands/categories 随迁）' },
  { path: 'db', layer: '基础设施', note: '数据库连接' },
  { path: 'signals.rs', layer: '基础设施', note: '信号映射（ADR-0044）' },
  { path: 'models', layer: '基础设施', note: '模型' },
  { path: 'error.rs', layer: '基础设施', note: '错误' },
  { path: 'settings.rs', layer: '基础设施', note: '设置' },
]

/** 壳层依赖形态：模块路径引用（crate::commands::x / commands::x）与别名引入 */
const SHELL_DEP_PATTERN = /\bcommands\s*::|\bcommands\s+as\b/

/** 测试豁免形态（ADR-0056 决策 5）：tests.rs 文件与 tests/ 目录 */
function isTestFile(relPath) {
  const segments = relPath.split('/')
  const file = segments[segments.length - 1]
  return file === 'tests.rs' || segments.slice(0, -1).includes('tests')
}

/**
 * 掩码 Rust 源文本中的注释与字符串/char 字面量：内容替换为等长空白
 * （保留换行与列位，行号不变），使依赖扫描只落在真实代码上。
 * 处理形态：行注释（//、///、//!）、块注释（/* .. *&#47;，可嵌套）、
 * 普通字符串（含转义）、原始字符串 r"…" / r#"…"#（多级 #）、
 * char 字面量（'a'、'\n'、'\u{…}'）；生命周期标注（'a）按非字面量处理。
 */
export function maskNonCode(text) {
  const out = text.split('')
  const n = text.length
  const blank = (from, to) => {
    for (let k = from; k < to && k < n; k++) if (out[k] !== '\n') out[k] = ' '
  }
  let i = 0
  while (i < n) {
    const c = text[i]
    if (c === '/' && text[i + 1] === '/') {
      // 行注释（含 /// 与 //!）到行尾
      const end = text.indexOf('\n', i)
      const stop = end === -1 ? n : end
      blank(i, stop)
      i = stop
    } else if (c === '/' && text[i + 1] === '*') {
      // 块注释，Rust 可嵌套
      let depth = 1
      let j = i + 2
      while (j < n && depth > 0) {
        if (text[j] === '/' && text[j + 1] === '*') {
          depth++
          j += 2
        } else if (text[j] === '*' && text[j + 1] === '/') {
          depth--
          j += 2
        } else {
          j++
        }
      }
      blank(i, j)
      i = j
    } else if (c === '"') {
      // 普通字符串：跳过转义对
      let j = i + 1
      while (j < n) {
        if (text[j] === '\\') j += 2
        else if (text[j] === '"') {
          j++
          break
        } else j++
      }
      blank(i, j)
      i = j
    } else if (c === 'r' && (text[i + 1] === '"' || (text[i + 1] === '#' && text[i + 2] === '"'))) {
      // 原始字符串 r"…" / r#"…"# / r##"…"##；前一字 符为标识符成分时是普通名字（如 for），不误伤
      const prev = i > 0 ? text[i - 1] : ''
      if (/[A-Za-z0-9_]/.test(prev)) {
        i++
        continue
      }
      let hashes = 0
      let j = i + 1
      while (text[j] === '#') {
        hashes++
        j++
      }
      const close = '"' + '#'.repeat(hashes)
      const end = text.indexOf(close, j + 1)
      const stop = end === -1 ? n : end + close.length
      blank(i, stop)
      i = stop
    } else if (c === "'") {
      // char 字面量 vs 生命周期：有闭引号为字面量，否则是生命周期标注（'a）
      let j = i + 1
      if (text[j] === '\\') {
        j++
        if (text[j] === '{') {
          const e = text.indexOf('}', j)
          j = e === -1 ? n : e + 1
        } else {
          j++
        }
      } else {
        j++
      }
      if (text[j] === "'") {
        const stop = j + 1
        blank(i, stop)
        i = stop
      } else {
        i++
      }
    } else {
      i++
    }
  }
  return out.join('')
}

/** 扫描单个 Rust 文本：返回命中壳层依赖的行号（1 起算）与原文 */
export function scanRustSource(text) {
  const hits = []
  const masked = maskNonCode(text)
  const maskedLines = masked.split('\n')
  const rawLines = text.split('\n')
  for (let i = 0; i < maskedLines.length; i++) {
    const m = maskedLines[i].match(SHELL_DEP_PATTERN)
    if (m) hits.push({ line: i + 1, text: rawLines[i].trim(), match: m[0] })
  }
  return hits
}

/** 递归收集目录下全部 .rs 文件（跳过测试豁免形态），相对路径排序保证输出确定 */
function collectRustFiles(dir, relBase) {
  const out = []
  for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name),
  )) {
    const abs = join(dir, entry.name)
    const rel = relBase ? `${relBase}/${entry.name}` : entry.name
    if (isTestFile(rel)) continue
    if (entry.isDirectory()) out.push(...collectRustFiles(abs, rel))
    else if (entry.name.endsWith('.rs')) out.push({ abs, rel })
  }
  return out
}

function main() {
  const repoRoot = fileURLToPath(new URL('..', import.meta.url))
  const srcDir = process.argv[2] ?? join(repoRoot, 'src-tauri', 'src')
  const problems = []
  let scannedFiles = 0
  const domainCount = WHITELIST.filter((w) => w.layer === '域目录').length

  for (const w of WHITELIST) {
    const abs = join(srcDir, w.path)
    let stat
    try {
      stat = statSync(abs)
    } catch {
      problems.push(`✗ 白名单路径不存在：${w.path}（${w.layer}：${w.note}）——目录改名/迁移后未同步守门清单`)
      continue
    }
    const files = stat.isDirectory() ? collectRustFiles(abs, w.path) : [{ abs, rel: w.path }]
    if (files.length === 0) {
      problems.push(
        `✗ 白名单条目扫不到非测试 Rust 文件：${w.path}（${w.layer}：${w.note}）——` +
          `全部是测试豁免形态或已空，清单与目录形状漂移`,
      )
      continue
    }
    scannedFiles += files.length
    for (const f of files) {
      for (const hit of scanRustSource(readFileSync(f.abs, 'utf8'))) {
        problems.push(
          `✗ 反向依赖：${w.path} 层（${w.note}）引用壳层 → ${f.rel}:${hit.line}（${hit.match}）\n` +
            `    ${hit.text}\n` +
            `    分层规则：壳 → 域 → 基础设施，域永不依赖壳（ADR-0056）；` +
            `被依赖逻辑应下沉到域目录或基础设施，或本次迁移应把该文件一并归位`,
        )
      }
    }
  }

  if (scannedFiles === 0) {
    problems.push('✗ 全部白名单条目扫不到任何非测试 Rust 文件——src 目录指错或白名单整体漂移，拒绝以空集假绿通过')
  }

  if (problems.length > 0) {
    for (const p of problems) console.error(p)
    console.error(
      `❌ 结构守门失败：${problems.length} 处问题` +
        `（分层规则：壳 → 域 → 基础设施，域永不依赖壳，白名单即规格，见 ADR-0056）`,
    )
    process.exit(1)
  }
  console.log(
    `✓ 结构守门：白名单 ${WHITELIST.length} 项（域目录 ${domainCount} + 基础设施 ${WHITELIST.length - domainCount}）` +
      `· 非测试文件 ${scannedFiles} 个 · 对壳层零依赖`,
  )
}

// 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的扫描函数。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
