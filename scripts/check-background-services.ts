#!/usr/bin/env bun
// 后台服务成对拉起守门（issue #961）：`backup::start_scheduler`（自动备份调度，
// 轮询同轮承载定时追补）与 `sync_engine::start_triggers`（同步触发编排，分平台
// 门收在域内一处，ADR-0098 决策 4）在**所有业务可用起点**必须成对拉起。
// 两调用各自独立书写时无任何机制保证成对——#863 会话已由同一根因造成两次
// 真实缺陷（分平台门漂移、`restart_app` 落 Ready 漏接同步触发），且「缺失一个
// 调用」不会让任何断言变红。守门规则（「白名单即规格」，ADR-0056 决策 4 哲学）：
//
// ① 这两个域入口的**生产调用**只允许出现在壳层唯一编排点 `lib.rs` 的
//    `start_background_services` 函数体内；其余位置命中即红——新加入口只调
//    其中一个（或干脆各写各的）必然在此变红。
// ② 编排点函数体内两个标识符必须**同时**出现（成对性在单点自证）；删掉
//    其一或整函数即红（「删除即变红」，与 #963 同一验收哲学）。
// ③ `sync_engine::start_sync_scheduler`（桌面轮询线程拉起）一并纳入守门：
//    它是分平台门的域内实现细节（仅 `start_triggers` 消费），直接调用即绕过
//    分平台门——#863 缺陷 1（解锁路径无门拉轮询线程）的形态，编排点函数体
//    内同样不放行。
// ④ 白名单条目（定义与域接缝再导出四个文件）必须存在且各自含其受守标识符
//    ——清单漂移 fail loud，防白名单烂掉后守门空转。
//
// 扫描边界：文本级扫描，形态同 check-structure.ts 家族——复用其注释与
// 字符串/char 字面量掩码（文档注释提到函数名不误报）；外挂测试模块/目录豁免
// （ADR-0056 决策 5），内联 #[cfg(test)] 不豁免；裸标识符 \b 边界匹配，
// `start_sync_scheduler` 等更长标识符不含更短名子串、天然不误伤；经别名改名
// 的间接引用文本不可达，靠评审兜底。
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-background-services.ts`。
// 默认校验本仓库；测试可传位置参数指向夹具：bun scripts/check-background-services.ts [src-dir]
// 挂载于 scripts/check.sh 质量门槛序列与 CI（build.yml frontend job）。

import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'
import { maskNonCode } from './check-structure.ts'

/** 唯一编排点：壳层文件（相对 src 根）与函数名（issue #961 单点） */
export const ORCHESTRATOR_FILE = 'lib.rs'
export const ORCHESTRATOR_FN = 'start_background_services'

/** 成对拉起名单（编排点函数体内必须同时出现的两个域入口） */
const PAIRED_NAMES = ['start_scheduler', 'start_triggers'] as const

/**
 * 受守标识符及其合法住址（导出供测试夹具派生，check-structure.test.ts 消费
 * 导出常量的同款先例）。wholeFile = 整文件豁免（定义/再导出/域内调用）；
 * orchestratorBodyAllowed = 唯一编排点函数体内是否放行（start_sync_scheduler
 * 不放行：它是分平台门的域内实现细节，出域即绕门）。
 */
export interface GuardedName {
  name: string
  wholeFile: readonly string[]
  orchestratorBodyAllowed: boolean
  note: string
}

export const GUARDED_NAMES: readonly GuardedName[] = [
  {
    name: 'start_scheduler',
    wholeFile: ['backup/auto.rs', 'backup/mod.rs'],
    orchestratorBodyAllowed: true,
    note: '自动备份调度入口（域定义 + 接缝再导出）',
  },
  {
    name: 'start_triggers',
    wholeFile: ['sync_engine/trigger/scheduler.rs', 'sync_engine/trigger/mod.rs', 'sync_engine/mod.rs'],
    orchestratorBodyAllowed: true,
    note: '同步触发编排单一入口（分平台门住址，ADR-0098 决策 4；issue #958 拆目录后定义住 trigger/scheduler.rs）',
  },
  {
    name: 'start_sync_scheduler',
    wholeFile: ['sync_engine/trigger/scheduler.rs', 'sync_engine/trigger/mod.rs', 'sync_engine/mod.rs'],
    orchestratorBodyAllowed: false,
    note: '桌面轮询线程拉起（仅 start_triggers 域内消费；直接调用即绕过分平台门，#863 缺陷 1 形态）',
  },
]

/** 受守标识符的裸名匹配形态（\b 边界；global 供 matchAll 逐行报出全部命中；
 *  任意限定路径与 use 引入均命中） */
const GUARDED_NAME_PATTERN = new RegExp(`\\b(?:${GUARDED_NAMES.map((g) => g.name).join('|')})\\b`, 'g')

/** 测试豁免形态（ADR-0056 决策 5，与 check-structure.ts 同款）：tests.rs 文件与
 *  tests/ 目录；内联 #[cfg(test)] 模块不豁免（更严，与家族一致）。 */
function isTestFile(relPath: string): boolean {
  const segments = relPath.split('/')
  const file = segments[segments.length - 1]
  return file === 'tests.rs' || segments.slice(0, -1).includes('tests')
}

/** 递归收集目录下全部非测试 .rs 文件，相对路径排序保证输出确定 */
function collectRustFiles(dir: string, relBase: string): { abs: string; rel: string }[] {
  const out: { abs: string; rel: string }[] = []
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

/** 编排点函数体在掩码文本中的行区间 [fnLine, closeLine]（1 起算，含端点）。
 *  起点为 `fn <name>` 所在行，终点为其后首个列 0 的 `}`（rustfmt 由 check.sh
 *  的 cargo fmt --check 保证，列 0 闭括号可靠）。 */
function orchestratorBodySpan(maskedLines: string[]): [number, number] | null {
  const fnLine = maskedLines.findIndex((l) => l.match(new RegExp(`fn\\s+${ORCHESTRATOR_FN}\\b`)))
  if (fnLine === -1) return null
  for (let i = fnLine + 1; i < maskedLines.length; i++) {
    if (maskedLines[i].trim() === '}') return [fnLine + 1, i + 1]
  }
  return null
}

/** 单条扫描命中：行号（1 起算）、原文行、命中标识符。
 *  与 check-structure.ts 的 scanRustSource（每行首个命中）不同：本守门按名
 *  核对合法住址，同一行（如 mod.rs 再导出行）可携带多个受守名，须全部报出。 */
interface NameHit {
  line: number
  text: string
  match: string
}

/** 扫描单个 Rust 文本（掩码注释与字符串/char 字面量）：返回全部受守名命中。
 *  同行同名多次出现只报一次（行号定位足够，不重复计数）。 */
function scanGuardedNames(rawLines: string[], maskedLines: string[]): NameHit[] {
  const hits: NameHit[] = []
  const seen = new Set<string>()
  for (let i = 0; i < maskedLines.length; i++) {
    for (const m of maskedLines[i].matchAll(GUARDED_NAME_PATTERN)) {
      const key = `${i + 1}:${m[0]}`
      if (seen.has(key)) continue
      seen.add(key)
      hits.push({ line: i + 1, text: rawLines[i].trim(), match: m[0] })
    }
  }
  return hits
}

function main(): void {
  const repoRoot = fileURLToPath(new URL('..', import.meta.url))
  const srcDir = process.argv[2] ?? join(repoRoot, 'src-tauri', 'src')
  const problems: string[] = []

  // 白名单存在性自证：每个整文件豁免条目必须存在且含其受守标识符（fail loud）
  for (const guarded of GUARDED_NAMES) {
    for (const relPath of guarded.wholeFile) {
      let source: string
      try {
        source = readFileSync(join(srcDir, relPath), 'utf8')
      } catch {
        problems.push(
          `✗ 白名单条目缺失：${relPath}（${guarded.name} 的 ${guarded.note}）——文件不存在，清单漂移 fail loud`,
        )
        continue
      }
      const hits = scanGuardedNames(source.split('\n'), maskNonCode(source).split('\n'))
      if (!hits.some((h) => h.match === guarded.name)) {
        problems.push(
          `✗ 白名单条目漂移：${relPath}（${guarded.note}）不再含标识符 \`${guarded.name}\`` +
            `——确认域入口改名/搬迁后同步更新本脚本白名单`,
        )
      }
    }
  }

  // 唯一编排点自证：函数必须存在，成对名单在其函数体内同时出现
  let orchestratorSpan: [number, number] | null = null
  try {
    const libSource = readFileSync(join(srcDir, ORCHESTRATOR_FILE), 'utf8')
    const maskedLines = maskNonCode(libSource).split('\n')
    orchestratorSpan = orchestratorBodySpan(maskedLines)
    if (orchestratorSpan === null) {
      problems.push(
        `✗ 唯一编排点缺失：${ORCHESTRATOR_FILE} 内找不到 \`fn ${ORCHESTRATOR_FN}\`` +
          `——成对拉起的单点被删或改名，业务可用起点将退回「各写各的」（issue #961 根因复现）`,
      )
    } else {
      const bodyRaw = libSource.split('\n').slice(orchestratorSpan[0] - 1, orchestratorSpan[1])
      const bodyMasked = maskedLines.slice(orchestratorSpan[0] - 1, orchestratorSpan[1])
      const bodyHits = new Set(scanGuardedNames(bodyRaw, bodyMasked).map((h) => h.match))
      for (const name of PAIRED_NAMES) {
        if (!bodyHits.has(name)) {
          problems.push(
            `✗ 成对性破坏：${ORCHESTRATOR_FILE} 的 \`${ORCHESTRATOR_FN}\` 函数体内缺少 \`${name}\`` +
              `——后台服务只拉一半，缺失一侧的业务在本会话静默失效（#863 两次缺陷的形态）`,
          )
        }
      }
    }
  } catch {
    problems.push(`✗ 唯一编排点文件缺失：${ORCHESTRATOR_FILE}——src 目录指错或壳层文件漂移`)
  }

  // 全树扫描：命中按名核对合法住址（整文件豁免 / 编排点函数体按名放行），其余一律红
  let files: { abs: string; rel: string }[] = []
  try {
    files = collectRustFiles(srcDir, '')
  } catch {
    if (problems.length === 0) {
      problems.push(`✗ 扫描根不可达：${srcDir}——拒绝以空集假绿通过`)
    }
  }
  for (const f of files) {
    const source = readFileSync(f.abs, 'utf8')
    const rawLines = source.split('\n')
    const maskedLines = maskNonCode(source).split('\n')
    for (const hit of scanGuardedNames(rawLines, maskedLines)) {
      const guarded = GUARDED_NAMES.find((g) => g.name === hit.match)
      if (guarded === undefined) continue // 形态自 GUARDED_NAMES 派生，不可达；防御性跳过
      if (guarded.wholeFile.includes(f.rel)) continue
      const inOrchestratorBody =
        f.rel === ORCHESTRATOR_FILE &&
        orchestratorSpan !== null &&
        hit.line >= orchestratorSpan[0] &&
        hit.line <= orchestratorSpan[1]
      if (inOrchestratorBody && guarded.orchestratorBodyAllowed) continue
      problems.push(
        `✗ 后台服务入口的生产调用脱离唯一编排点：${f.rel}:${hit.line}（${hit.match}）\n` +
          `    ${hit.text}\n` +
          `    规则：\`${guarded.name}\`——${guarded.note}；\n` +
          `    自动备份与同步触发必须经 \`${ORCHESTRATOR_FN}\`（${ORCHESTRATOR_FILE}）成对拉起` +
          `（issue #961），新增业务可用起点请调用该编排点，不要单独调用任一域入口`,
      )
    }
  }

  if (problems.length > 0) {
    for (const p of problems) console.error(p)
    console.error(
      `❌ 后台服务成对拉起守门失败：${problems.length} 处问题` +
        `（单点编排 + 白名单即规格，见 issue #961 / ADR-0056 决策 4 哲学）`,
    )
    process.exit(1)
  }
  const pathCount = new Set(GUARDED_NAMES.flatMap((g) => [...g.wholeFile])).size
  console.log(
    `✓ 后台服务成对拉起守门：受守入口 ${GUARDED_NAMES.length} 个` +
      `（${GUARDED_NAMES.map((g) => g.name).join(' / ')}）· 白名单路径 ${pathCount} 个（定义与再导出）` +
      ` · 生产调用收敛于 \`${ORCHESTRATOR_FN}\` 单点 · 全树扫描 ${files.length} 个非测试文件零脱离`,
  )
}

// 仅直接运行时执行 main；被其他工具 import 时只取导出的扫描逻辑。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
