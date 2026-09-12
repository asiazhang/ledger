#!/usr/bin/env bun
// 前端异步守门（issue #1039，#1008 决议 4 follow-up）：两条源码扫描门。
//
// 背景：手搓异步守卫在 ADR-0040 当点（2026-08-30）全仓仅搜索视图一处，架构走查
// （2026-09-11）已增殖到三处——「新代码一律走 Loadable」纯纪律守不住。#1008 把存量
// 六处收编进 useLoadable 后，手搓竞态序号已在源码面归零；本门把约束固化成可执行
// 检查（仿 scripts/check-structure.ts 思路，Bun 运行时 ADR-0083），进 check.sh 与 CI。
//
// 规则 1（硬零容忍）：手搓竞态序号 `let <名>seq = 0` 形态即红。竞态序号唯一合法
// 住址 composables/useLoadable.ts（接缝本体，#1008 收编后的单点）——与原生事务语句
// 禁令的「唯一合法住址」同款纪律（check-structure / #1014）。形态较 #1039 票面正则
// 加宽一处：前缀 `\w+` → `\w*`，裸名 `let seq = 0` 同样识别（票面口径下零误伤，
// 实测仅接缝本体命中）；命名尾部通配（seqNum 等）与非 let 形态不在检测面，靠评审兜底。
//
// 规则 2（基线冻结、只减不增）：catch 内直弹 toast 模板
// `message.error(t(…errorMessage…))` 不能零容忍——#1039 交付时实测 61 处 / 31 文件
// （#1008 交付分支票面记 60 / 31，以本票实现时重测为准）。TOAST_BASELINE 按文件
// 冻结存量计数，现计数与基线全等：新增/回潮（>）红；收缩未同步（<）同样红并提示
// 下调基线——与样式块白名单「迁移一个删一个」同一纪律（基线即规格，不留陈目）。
// 存量收编本身（连带 i18n 键与插值名清理）另立专项（#1039 Out of Scope），本门只封增量。
//
// 扫描边界：src 下 .ts/.vue 文本级行扫描（含 __tests__，当前零命中；src 现无其他
// 源码扩展名，新增须同步扫描面）；行首注释行（// /* * <!--）跳过——注释提及靶形态
// 不误报；多行块注释内部行与跨行调用形态不可达，靠评审兜底。守门自身包装测试已随
// 测试归位迁到 scripts/（issue #1158），在扫描根之外，无需文件级豁免（原豁免已随
// 迁移删除）。不设注释豁免通道：逃逸 = 显式回 issue/ADR 讨论后改脚本
// （check-style-blocks 同一纪律）。
//
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-async-guards.ts`。
// 默认扫描本仓库 src；测试可传位置参数指向夹具目录：bun scripts/check-async-guards.ts [scan-root]
// 挂载于 scripts/check.sh 质量门槛序列与 CI（build.yml frontend job）。

import { readdirSync, readFileSync } from 'node:fs'
import { join, relative } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'

/** 规则 1 形态：手搓竞态序号（`\w*` 含裸名 seq；`\b` 防吞后缀，seqNum 不在检测面） */
const HAND_ROLLED_SEQ_PATTERN = /\blet\s+\w*[Ss]eq\s*=\s*0\b/

/** 规则 1 唯一合法住址（相对扫描根路径）：useLoadable 接缝本体（#1008 收编后的
 *  竞态守卫单点，其内部 `let seq = 0` 是实现细节而非手搓守卫） */
const SEQ_SEAM_FILE = 'composables/useLoadable.ts'

/** 规则 2 形态：catch 内直弹 toast 模板（单行；errorMessage 为 utils/errors 统一错误提取） */
const CATCH_TOAST_PATTERN = /message\.error\(\s*t\([^\n]*errorMessage/

/**
 * 规则 2 存量基线（#1039 交付时点实测快照：61 处 / 31 文件；键相对扫描根、posix
 * 分隔，值为该文件允许的命中行数）。现计数须与基线全等：新增红、收缩未同步同样红
 * ——收编一处下调一处，收编至零删除条目；基线条目指向的文件不可达亦红（清单漂移
 * fail loud）。新增条目不允许：基线只减不增。
 */
export const TOAST_BASELINE: Readonly<Record<string, number>> = {
  'components/AddItemForm.vue': 1,
  'components/NoteCopyButton.vue': 1,
  'components/PhysicalAssetDisposeModal.vue': 1,
  'components/PhysicalAssetFormModal.vue': 1,
  'components/PhysicalAssetValuationModal.vue': 1,
  'components/PolicyAgreementSection.vue': 2,
  'components/PolicyFormModal.vue': 2,
  'components/categories/CategoryAddForm.vue': 1,
  'components/categories/CategoryEditModal.vue': 1,
  'components/categories/CategoryTree.vue': 2,
  'components/scheduled/PlanDetailModal.vue': 2,
  'components/scheduled/SubscriptionSpendPanel.vue': 1,
  'components/scheduled/SubscriptionsPane.vue': 1,
  'components/settings/AboutSettings.vue': 1,
  'components/settings/BaseCurrencySettings.vue': 2,
  'components/settings/DataLocationSettings.vue': 3,
  'components/settings/LogSettings.vue': 2,
  'components/settings/SearchDataSettings.vue': 1,
  'components/settings/SyncSettings.vue': 6,
  'composables/useBackup.ts': 6,
  'composables/useRefundForm.ts': 1,
  'composables/useRestoreFromFile.ts': 2,
  'composables/useScheduledPlanForm.ts': 1,
  'composables/useScheduledPlanList.ts': 1,
  'composables/useTransactionModalState.ts': 2,
  'views/AccountsView.vue': 4,
  'views/AiPromptView.vue': 2,
  'views/BudgetView.vue': 3,
  'views/ItemsView.vue': 4,
  'views/PoliciesView.vue': 1,
  'views/TransactionsView.vue': 2,
}

/** 行首注释形态：整行跳过（行内尾注与多行块注释内部行不可达，靠评审兜底） */
function isCommentLine(trimmed: string): boolean {
  return (
    trimmed.startsWith('//') ||
    trimmed.startsWith('/*') ||
    trimmed.startsWith('*') ||
    trimmed.startsWith('<!--')
  )
}

interface SourceFileRef {
  abs: string
  rel: string
}

/** 递归收集扫描根下全部 .ts/.vue 文件（按相对路径排序保证输出确定） */
function collectSourceFiles(root: string): SourceFileRef[] {
  const out: SourceFileRef[] = []
  const visit = (dir: string): void => {
    for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) =>
      a.name.localeCompare(b.name),
    )) {
      const abs = join(dir, entry.name)
      if (entry.isDirectory()) visit(abs)
      else if (entry.name.endsWith('.ts') || entry.name.endsWith('.vue')) {
        const rel = relative(root, abs).split('\\').join('/')
        out.push({ abs, rel })
      }
    }
  }
  visit(root)
  return out
}

interface LineHit {
  line: number
  text: string
}

/** 行级扫描单文件：返回命中靶形态的行号（1 起算）与原文行；行首注释行跳过 */
function scanLines(source: string, pattern: RegExp): LineHit[] {
  const hits: LineHit[] = []
  source.split('\n').forEach((raw, i) => {
    const trimmed = raw.trim()
    if (isCommentLine(trimmed)) return
    if (pattern.test(raw)) hits.push({ line: i + 1, text: trimmed })
  })
  return hits
}

function main(): void {
  const scanRoot = process.argv[2] ?? fileURLToPath(new URL('../src', import.meta.url))
  const files = collectSourceFiles(scanRoot)

  const problems: string[] = []
  if (files.length === 0) {
    problems.push(
      '✗ 扫描根提不出任何 .ts/.vue 源文件——目录指错或源码整体漂移，拒绝以空集假绿通过',
    )
  }

  // 规则 1：手搓竞态序号，硬零容忍（唯一合法住址豁免）
  let seqHits = 0
  // 规则 2：按文件计数，与基线全等校验
  const toastCounts = new Map<string, number>()
  const toastLinesByFile = new Map<string, string[]>()
  for (const f of files) {
    const source = readFileSync(f.abs, 'utf8')
    if (f.rel !== SEQ_SEAM_FILE) {
      for (const hit of scanLines(source, HAND_ROLLED_SEQ_PATTERN)) {
        seqHits++
        problems.push(
          `✗ 手搓竞态序号：${f.rel}:${hit.line}（${hit.text}）\n` +
            `    竞态守卫一律走 useLoadable 接缝（#1008：后发覆盖先发、迟到结果作废与\n` +
            `    loading/错误收尾一体化）；手搓序号已归零，此处为回潮——唯一合法住址 ` +
            `${SEQ_SEAM_FILE}（接缝本体，#1039 规则 1）`,
        )
      }
    }
    const hits = scanLines(source, CATCH_TOAST_PATTERN)
    if (hits.length > 0) {
      toastCounts.set(f.rel, hits.length)
      toastLinesByFile.set(f.rel, hits.map((h) => `${h.line}`))
    }
  }

  // 规则 2：现计数 vs 基线，全等校验（新增红 / 收缩未同步红 / 条目不可达红）
  let toastMismatches = 0
  const scannedToastTotal = [...toastCounts.values()].reduce((a, b) => a + b, 0)
  for (const f of files) {
    const current = toastCounts.get(f.rel) ?? 0
    const baseline = TOAST_BASELINE[f.rel] ?? 0
    if (current > baseline) {
      toastMismatches++
      problems.push(
        `✗ 直弹 toast 回潮/新增：${f.rel} 现 ${current} 处 > 基线 ${baseline} 处（行 ` +
          `${(toastLinesByFile.get(f.rel) ?? []).join('、')}）——错误反馈走 useLoadable ` +
          `error 通道（showErrorToast 单点，#1008）；存量收编属专项范围（#1039 Out of Scope）`,
      )
    } else if (current < baseline) {
      toastMismatches++
      problems.push(
        `✗ 直弹 toast 基线待收缩：${f.rel} 现 ${current} 处 < 基线 ${baseline} 处——` +
          `存量已收编，请同步下调/删除 scripts/check-async-guards.ts 的 TOAST_BASELINE 条目` +
          `（基线即规格、只减不增、不留陈目，#1039 规则 2）`,
      )
    }
  }
  const scannedRels = new Set(files.map((f) => f.rel))
  for (const rel of Object.keys(TOAST_BASELINE)) {
    if (!scannedRels.has(rel)) {
      toastMismatches++
      problems.push(
        `✗ toast 基线条目不可达：${rel}——文件删除/改名后未同步 TOAST_BASELINE` +
          `（清单漂移 fail loud，#1039 规则 2）`,
      )
    }
  }

  if (problems.length > 0) {
    for (const p of problems) console.error(p)
    console.error(
      `❌ 异步守门失败：手搓竞态序号 ${seqHits} 处（唯一合法住址 ${SEQ_SEAM_FILE}）· ` +
        `toast 基线失配 ${toastMismatches} 处（基线 ${Object.keys(TOAST_BASELINE).length} 文件）` +
        `——手搓异步守卫一律走 Loadable（#1008 / #1039，ADR-0040 当点一处 → 走查三处的教训）`,
    )
    process.exit(1)
  }
  console.log(
    `✅ 异步守门：${files.length} 个前端源文件 · 手搓竞态序号 0（唯一合法住址 ${SEQ_SEAM_FILE}）· ` +
      `catch 直弹 toast 基线全等（${toastCounts.size} 文件 ${scannedToastTotal} 处，只减不增，#1039）`,
  )
}

// 仅直接运行时执行 main；被测试 import 时只取导出的基线清单（夹具派生单一事实源）。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
