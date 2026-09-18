#!/usr/bin/env bun
// 样式块守门（issue #888 / ADR-0093）：`src` 下 .vue 文件禁止携带 <style> 块
// （scoped / 非 scoped / module 一律算），存量携带者以下方白名单冻结——白名单外
// 新增 <style> 块即拦截。新样式一律走组件旁路 vanilla-extract 样式文件
// （*.css.ts，与组件同目录共置），存量随触碰渐进迁移（boy-scout），迁移后把
// 文件从白名单移除。白名单是登记表而非备忘录，双向核验：白名单外的 <style>
// 块即红（正向），白名单条目指向的文件在扫描面内不可达同样即红——文件删除/
// 改名/搬迁后未同步清单时陈目不静默存留（#1360，清单漂移 fail loud，同
// TOAST_BASELINE 不可达检查与结构守门规则⑥⑦形制）。
// 不设注释豁免通道：逃逸 = 显式回 ADR 讨论后改脚本（与 check-dialog-forms.ts
// 同一纪律）。样式类型安全由 vue-tsc / Vite 构建期把守，本脚本只守样式块纪律。
// TypeScript 化 + Bun 运行时（issue #734 / ADR-0083）：类型经 tsconfig.scripts.json
// 门槛检查；调用方式 `bun scripts/check-style-blocks.ts`。
// 默认扫描本仓库 src；可传位置参数指向其他目录：bun scripts/check-style-blocks.ts [scan-root]
// 挂载于 scripts/check.sh 质量门槛序列；CI 无独立 workflow 步骤，经 vitest 测试
// 分片的真实仓绿基线用例可见（scripts/check-style-blocks.test.ts，issue #1473）。

import { readdirSync, readFileSync } from 'node:fs'
import { join, relative } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'
import { parse } from 'vue/compiler-sfc'

/** 存量 <style> 块白名单（issue #888 交付时点快照，按路径排序；#1159 起源码按域
 *  归位，域文件路径同步为域目录坐标 src/<域>/）：
 *  试点组件 AiPromptView.vue 已迁出，不在此列；其余随触碰渐进迁移，
 *  迁移一个删一个，不允许只增不减；条目指向的文件必须可达，不可达即红（#1360）。
 *  路径以仓库根为基准、正斜杠分隔。导出供包装测试夹具派生（单一事实源，
 *  TOAST_BASELINE 同款纪律）。 */
export const STYLE_BLOCK_WHITELIST: readonly string[] = [
  'src/App.vue',
  'src/accounts/AccountLink.vue',
  'src/backup/StartupFailureScreen.vue',
  'src/backup/UnlockScreen.vue',
  'src/components/GlobalBusyBar.vue',
  'src/components/MobileNavShell.vue',
  'src/components/QuickTimeRange.vue',
  'src/investment/PortfolioTrendPanel.vue',
  'src/merchants/MerchantLink.vue',
  'src/physical-asset/PhysicalAssetDisposeModal.vue',
  'src/physical-asset/PhysicalAssetValuationModal.vue',
  'src/policy/PolicyFormModal.vue',
  'src/reports/MerchantRankingPanel.vue',
  'src/scheduled/PlanDetailModal.vue',
  'src/scheduled/SubscriptionSpendPanel.vue',
  'src/settings/BookSidebarEntry.vue',
  'src/settings/EncryptionSettings.vue',
  'src/settings/PassphraseStrengthMeter.vue',
  'src/transaction/AmountCell.vue',
  'src/transaction/SourceLink.vue',
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
  // 反向存在性断言（#1360）：白名单条目必须在扫描面内可达，陈目即红
  const scannedRels = new Set(files.map((file) => normalizePath(file)))
  const unreachable = STYLE_BLOCK_WHITELIST.filter((rel) => !scannedRels.has(rel))
  if (violations.length === 0 && failures.length === 0 && unreachable.length === 0) {
    console.log(
      `✅ 样式块守门：${files.length} 个 .vue 文件检查通过——白名单（${STYLE_BLOCK_WHITELIST.length} 个存量文件）外零 <style> 块且全条目可达（ADR-0093 / #1360）`,
    )
    return
  }
  for (const f of failures) console.error(f)
  for (const rel of unreachable) {
    console.error(
      `✗ 白名单条目不可达：${rel}——文件删除/改名/搬迁后未同步 STYLE_BLOCK_WHITELIST` +
        `（清单漂移 fail loud，#1360；请同步删除或改写该条目，陈目不静默存留）`,
    )
  }
  for (const v of violations) {
    console.error(
      `✗ 新增 <style> 块：${v}\n  样式方案守门（ADR-0093 / issue #888）：白名单外禁止 <style> 块，` +
        `新样式一律写组件旁路 vanilla-extract 样式文件（*.css.ts，与组件同目录共置）。` +
        `若属存量文件迁移回退或清单漂移，请同步 scripts/check-style-blocks.ts 的 STYLE_BLOCK_WHITELIST。`,
    )
  }
  process.exit(1)
}

// 仅直接运行时执行 main；导出的检查函数与白名单清单供包装测试/后续工具复用。
// 包装测试随 #1360 补齐（scripts/check-style-blocks.test.ts，spawnSync 进程级
// 断言退出码与输出，同 check-async-guards.test.ts 形制），挂 check.sh 随 CI 执行。
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
