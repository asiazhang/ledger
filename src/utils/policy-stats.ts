import { formatAmount } from '@ledger/money'
import type { Currency, PolicyStats } from '@ledger/types'

/**
 * 保单视角统计的展示取值辅助（issue #363 / ADR-0051 决策 6）：
 * 列表列与编辑弹窗（详情）摘要共用同一取值口径——统计行缺失（加载窗口）
 * 显示占位，合计按自带折算基准币种经 formatAmount 展示，不做本地二次聚合。
 *
 * 币种解析由消费方注入（issue #1156 归位）：原生币种码到 Currency 的映射
 * 单一来源在参考数据 store，本模块保持纯函数（不上行引用 stores）。
 */

/** 统计行的本位币合计展示文本（累计已缴 / 累计流入共用，pick 取对应字段）。 */
export function policyStatAmountText(
  stats: PolicyStats | null | undefined,
  pick: (s: PolicyStats) => number,
  getCurrency: (code: string) => Currency | undefined,
): string {
  if (!stats) return '—'
  return formatAmount(pick(stats), getCurrency(stats.native_currency))
}
