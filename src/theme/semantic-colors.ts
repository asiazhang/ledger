import type { Theme } from '@/stores/app'
import type { TransactionKind } from '@/types'

/**
 * 交易类型语义色（issue #435）——金额业务色的**单一来源**。
 *
 * 六种交易类型（支出/收入/转账/退款/买入/卖出）各一个专属色，
 * 每色亮/暗两套色值，随外观主题（Appearance）切换。消费方：
 * - 交易列工厂（`src/components/transaction-columns.ts`，交易列表与搜索结果共用）
 *   的金额单元格，运行时响应式读取主题取色；
 * - 报表页月度收支图（ReportsView）收入/支出/退款三根语义色柱，与列表同源。
 *
 * 约定：
 * - 表驱动穷尽（`Record<TransactionKind, …>`）：新增交易类型时此处编译报错，
 *   强制补色，不出现无色回退。
 * - 暗色为默认主题，暗色变体取亮色同色相的提亮版，保证近黑底可读。
 * - 与类型标签色（NTag：收入 success、支出 warning、退款 info、其余 default）
 *   及品牌琥珀强调色（账户/商户链接）相互独立：借出/借入/收回/还款是 transfer
 *   的派生视角（ADR-0053），金额与普通转账同为紫色，不做派生级区分。
 * - 值只在本模块出现：列表与图表不再硬编码色值（分类构成图等按 id 散列的
 *   任意配色不属于语义色，不归本模块）。
 */
export interface SemanticColor {
  light: string
  dark: string
}

export const KIND_SEMANTIC_COLORS: Record<TransactionKind, SemanticColor> = {
  expense: { light: '#d03050', dark: '#e88080' },
  income: { light: '#18a058', dark: '#63e2b7' },
  refund: { light: '#2080f0', dark: '#63a8f2' },
  transfer: { light: '#722ed1', dark: '#b37feb' },
  buy: { light: '#eb2f96', dark: '#ff85c0' },
  sell: { light: '#13c2c2', dark: '#5cdbd3' },
}

/** 语义色覆盖的交易类型闭集（与交易类型闭集同源，运行时校验锚点）。 */
export const SEMANTIC_COLOR_KINDS = Object.keys(KIND_SEMANTIC_COLORS) as TransactionKind[]

/** 按交易类型与当前主题取语义色（纯选择器，随主题响应式消费）。 */
export function kindSemanticColor(kind: TransactionKind, theme: Theme): string {
  return KIND_SEMANTIC_COLORS[kind][theme]
}
