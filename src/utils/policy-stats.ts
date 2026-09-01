import { formatAmount } from '@/types'
import { useReferenceStore } from '@/stores/reference'
import type { PolicyStats } from '@/types'

/**
 * 保单视角统计的展示取值辅助（issue #363 / ADR-0051 决策 6）：
 * 列表列与编辑弹窗（详情）摘要共用同一取值口径——统计行缺失（加载窗口）
 * 显示占位，合计按自带折算基准币种经 formatAmount 展示，不做本地二次聚合。
 */

/** 统计行的本位币合计展示文本（累计已缴 / 累计流入共用，pick 取对应字段）。 */
export function policyStatAmountText(
  stats: PolicyStats | null | undefined,
  pick: (s: PolicyStats) => number,
): string {
  if (!stats) return '—'
  const currency = useReferenceStore().getCurrency(stats.native_currency)
  return formatAmount(pick(stats), currency)
}
