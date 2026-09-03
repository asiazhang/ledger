#!/usr/bin/env node
// 结构守门（issue #396 / ADR-0056；模型域化禁令 issue #424 / ADR-0059 决策 6）：
// 白名单式分层依赖检查。
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
// 模型域化禁令（ADR-0059 T7 / #424 收口落地，全树扫描、同样掩码与测试豁免）：
// ① 全局模型模块路径残留禁令——`crate::models` / `tauri_app_lib::models` 即红：
//    全局模型目录已随域归位消亡，防扁平命名空间复活（crate 根裸路径 `models::x`
//    与别名改写文本不可达，靠评审兜底）；
// ② 域模型 glob 再导出禁令——`pub use …model(s)::*`（域接缝或跨域拍平）与
//    域模型文件（model.rs / models.rs）内的 `pub use …::*` 聚合即红，
//    所有权必须逐类型可见（`pub(crate) use` 受限再导出与私有 `use` glob 引入
//    不在文本可辨范围，靠评审兜底）。
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
  { path: 'physical_asset', layer: '域目录', note: '实物资产域（issue #466 新建即归位，ADR-0064）' },
  { path: 'investment', layer: '域目录', note: '投资域（#401 阶段 5 归位，主体自 commands/investment 随迁；价格写入单点自 sync/persist 迁入）' },
  { path: 'accounts', layer: '域目录', note: '账户域（#404 参考数据域归位，主体自 commands/accounts 随迁）' },
  { path: 'categories', layer: '域目录', note: '分类域（#404 参考数据域归位，主体自 commands/categories 随迁）' },
  { path: 'currencies', layer: '域目录', note: '币种域（#404 参考数据域归位，清单查询自 commands/currencies 迁入）' },
  { path: 'reports', layer: '域目录', note: '报表域（#405 归位，月度汇总/分类/商户/日期极值聚合读模型，消费 transaction::amount 矩阵）' },
  { path: 'dashboard', layer: '域目录', note: '仪表盘域（#405 归位，全仓净资产跨币种折算聚合）' },
  { path: 'backup', layer: '域目录', note: '备份域（#406 归位，备份引擎自 commands/backup/core、自动备份调度自顶层 auto_backup.rs 整合随迁）' },
  { path: 'sync', layer: '域目录', note: '行情同步域（#407 归位，HTTP 爬取/东财基金净值/增全量同步编排自 commands/sync 随迁）' },
  { path: 'db', layer: '基础设施', note: '数据库连接' },
  { path: 'signals.rs', layer: '基础设施', note: '信号映射（ADR-0044）' },
  { path: 'error.rs', layer: '基础设施', note: '错误' },
  { path: 'settings.rs', layer: '基础设施', note: '设置' },
  { path: 'fs_util.rs', layer: '基础设施', note: '文件级原子操作工具（备份与 DataLocation 搬迁共用，#408 纳入守门）' },
  { path: 'logger.rs', layer: '基础设施', note: '日志初始化与滚动清理（#408 纳入守门）' },
  { path: 'events.rs', layer: '基础设施', note: '事件发射机制（ADR-0054，#408 纳入守门）' },
]

/** 壳层依赖形态：模块路径引用（crate::commands::x / commands::x）与别名引入 */
const SHELL_DEP_PATTERN = /\bcommands\s*::|\bcommands\s+as\b/

/** 规则①形态：全局模型模块路径（全局目录已消亡，任何引用即残留） */
const GLOBAL_MODEL_PATH_PATTERN = /\b(?:crate|tauri_app_lib)\s*::\s*models\b/

/** 规则②形态：模型模块的 glob 再导出——域接缝 `pub use model::*` 与
 *  跨域/旧目录同名拍平 `pub use …::models::*`；逐类型花括号列举不命中 */
const MODEL_GLOB_REEXPORT_PATTERN = /\bpub\s+use\s+[\w:]*\bmodels?\b\s*::\s*\*/

/** 规则②形态：任意 glob 再导出（仅用于域模型文件内的聚合扫描） */
const MODEL_FILE_GLOB_PATTERN = /\bpub\s+use\s+[\w:]*\*/

/** 域模型文件（ADR-0059 目标形状：每域一个 model.rs；定时计划域为先例名 models.rs） */
function isModelFile(relPath) {
  const segments = relPath.split('/')
  const file = segments[segments.length - 1]
  return file === 'model.rs' || file === 'models.rs'
}

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

/** 扫描单个 Rust 文本（掩码注释与字符串/char 字面量）：返回命中指定形态的
 *  行号（1 起算）与原文；形态缺省为壳层依赖（白名单分层检查的既有行为） */
export function scanRustSource(text, pattern = SHELL_DEP_PATTERN) {
  const hits = []
  const masked = maskNonCode(text)
  const maskedLines = masked.split('\n')
  const rawLines = text.split('\n')
  for (let i = 0; i < maskedLines.length; i++) {
    const m = maskedLines[i].match(pattern)
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

  // 模型域化禁令（规则①/②）：全树扫描（壳、域、基础设施、顶层文件），
  // 残留引用可出现在任何层；collectRustFiles 自带测试豁免（ADR-0056 决策 5）。
  // srcDir 整体不可达时静默交由白名单循环报「路径不存在」，不在此抛栈。
  let allFiles = []
  try {
    allFiles = collectRustFiles(srcDir, '')
  } catch {
    // 目录缺失：白名单循环会逐条报错并 fail loud
  }
  for (const f of allFiles) {
    const source = readFileSync(f.abs, 'utf8')
    for (const hit of scanRustSource(source, GLOBAL_MODEL_PATH_PATTERN)) {
      problems.push(
        `✗ 全局模型路径残留：${f.rel}:${hit.line}（${hit.match}）\n` +
          `    ${hit.text}\n` +
          `    全局模型目录已随 ADR-0059 模型域化消亡（T7 / #424），` +
          `模型类型一律走域路径显式 import（如 crate::transaction::model::Transaction）` +
          `——防扁平命名空间复活`,
      )
    }
    for (const hit of scanRustSource(source, MODEL_GLOB_REEXPORT_PATTERN)) {
      problems.push(
        `✗ 域模型 glob 再导出：${f.rel}:${hit.line}（${hit.match}）\n` +
          `    ${hit.text}\n` +
          `    域 model 只许逐类型再导出，所有权必须逐类型可见 ` +
          `（ADR-0059 决策 3/6，#424）：改为 pub use model::{TypeA, TypeB} 形态`,
      )
    }
    if (isModelFile(f.rel)) {
      for (const hit of scanRustSource(source, MODEL_FILE_GLOB_PATTERN)) {
        problems.push(
          `✗ 域模型文件内 glob 聚合：${f.rel}:${hit.line}（${hit.match}）\n` +
            `    ${hit.text}\n` +
            `    域模型文件只承载本域类型定义与逐类型再导出，禁止 glob 聚合 ` +
            `（ADR-0059 决策 3/6，#424）`,
        )
      }
    }
  }

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
      `· 白名单面非测试文件 ${scannedFiles} 个 · 对壳层零依赖` +
      `· 模型域化禁令全树扫描 ${allFiles.length} 个文件零残留（ADR-0059）`,
  )
}

// 仅直接运行时执行 main；被测试/其他工具 import 时只取导出的扫描函数。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
