#!/usr/bin/env bun
// 样式块守门（issue #888 / ADR-0093）：`src` 下 .vue 文件禁止携带 <style> 块
// （scoped / 非 scoped / module 一律算），存量携带者以下方白名单冻结——白名单外
// 新增 <style> 块即拦截。新样式一律走组件旁路 vanilla-extract 样式文件
// （*.css.ts，与组件同目录共置），存量随触碰渐进迁移（boy-scout），迁移后把
// 文件从白名单移除。
// 不设注释豁免通道：逃逸 = 显式回 ADR 讨论后改脚本（与 check-dialog-forms.ts
// 同一纪律）。样式类型安全由 vue-tsc / Vite 构建期把守，本脚本只守样式块纪律。
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-style-blocks.ts`。
// 默认扫描本仓库 src；可传位置参数指向其他目录：bun scripts/check-style-blocks.ts [scan-root]
// 挂载于 scripts/check.sh 质量门槛序列与 CI。

import { readdirSync, readFileSync } from 'node:fs'
import { join, relative } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'
import { parse } from 'vue/compiler-sfc'

/** 存量 <style> 块白名单（issue #888 交付时点快照，按路径排序）：
 *  试点组件 AiPromptView.vue 已迁出，不在此列；其余随触碰渐进迁移，
 *  迁移一个删一个，不允许只增不减。路径以仓库根为基准、正斜杠分隔。 */
const STYLE_BLOCK_WHITELIST: string[] = [
  'src/App.vue',
  'src/components/AccountLink.vue',
  'src/components/AmountCell.vue',
  'src/components/BookSidebarEntry.vue',
  'src/components/GlobalBusyBar.vue',
  'src/components/MerchantLink.vue',
  'src/components/MobileNavShell.vue',
  'src/components/PhysicalAssetDisposeModal.vue',
  'src/components/PhysicalAssetValuationModal.vue',
  'src/components/PolicyFormModal.vue',
  'src/components/QuickTimeRange.vue',
  'src/components/SourceLink.vue',
  'src/components/StartupFailureScreen.vue',
  'src/components/UnlockScreen.vue',
  'src/components/investments/PortfolioTrendPanel.vue',
  'src/components/reports/MerchantRankingPanel.vue',
  'src/components/scheduled/PlanDetailModal.vue',
  'src/components/scheduled/SubscriptionSpendPanel.vue',
  'src/components/settings/EncryptionSettings.vue',
  'src/components/settings/PassphraseStrengthMeter.vue',
  'src/views/GroupMoreView.vue',
  'src/views/InvestmentsView.vue',
  'src/views/ItemsView.vue',
  'src/views/PoliciesView.vue',
  'src/views/ReportsView.vue',
  'src/views/ScheduledView.vue',
  'src/views/SettingsView.vue',
]

/** 白名单归一化：统一为仓库根相对、正斜杠分隔的路径。 */
function normalizePath(file: string): string {
  return relative(process.cwd(), file).split('\\').join('/')
}

/** 递归收集扫描根下全部 .vue 文件（按名排序保证输出稳定）。 */
export function collectVueFiles(root: string): string[] {
  const out: string[] = []
  const visit = (dir: string): void => {
    for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) =>
      a.name.localeCompare(b.name),
    )) {
      const full = join(dir, entry.name)
      if (entry.isDirectory()) visit(full)
      else if (entry.name.endsWith('.vue')) out.push(full)
    }
  }
  visit(root)
  return out
}

/** 检查单个 .vue 文件：返回是否携带 <style> 块与解析失败信息（如有）。
 *  经 vue/compiler-sfc 解析（注释里的 "<style>" 字样不误报）。 */
export function checkVueFile(file: string): { hasStyleBlock: boolean; failure?: string } {
  const source = readFileSync(file, 'utf-8')
  const { descriptor, errors } = parse(source, { filename: file })
  if (errors.length > 0) {
    return { hasStyleBlock: false, failure: errors.map((e) => e.message).join('; ') }
  }
  return { hasStyleBlock: descriptor.styles.length > 0 }
}

function main(): void {
  const scanRoot = process.argv[2] ?? fileURLToPath(new URL('../src', import.meta.url))
  const files = collectVueFiles(scanRoot)
  const violations: string[] = []
  const failures: string[] = []
  for (const file of files) {
    const r = checkVueFile(file)
    if (r.failure) {
      failures.push(`✗ SFC 解析失败：${normalizePath(file) || file} — ${r.failure}`)
      continue
    }
    if (r.hasStyleBlock && !STYLE_BLOCK_WHITELIST.includes(normalizePath(file))) {
      violations.push(normalizePath(file))
    }
  }
  if (violations.length === 0 && failures.length === 0) {
    console.log(
      `✅ 样式块守门：${files.length} 个 .vue 文件检查通过——白名单（${STYLE_BLOCK_WHITELIST.length} 个存量文件）外零 <style> 块（ADR-0093）`,
    )
    return
  }
  for (const f of failures) console.error(f)
  for (const v of violations) {
    console.error(
      `✗ 新增 <style> 块：${v}\n  样式方案守门（ADR-0093 / issue #888）：白名单外禁止 <style> 块，` +
        `新样式一律写组件旁路 vanilla-extract 样式文件（*.css.ts，与组件同目录共置）。` +
        `若属存量文件迁移回退或清单漂移，请同步 scripts/check-style-blocks.ts 的 STYLE_BLOCK_WHITELIST。`,
    )
  }
  process.exit(1)
}

// 仅直接运行时执行 main；导出的检查函数供其他脚本/后续工具复用。按 issue #888
// 测试决策，本守门不写单测——脚本运行即检查，挂入 check.sh 随 CI 执行。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
