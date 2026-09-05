// 商户消费排行横向柱状图的数据形态（issue #588）：排序与 topN 截断已收口后端
// `merchant_shares`，前端按返回序渲染、零口径逻辑；名次梯度色（同色系、第 1 名
// 最深）收纯函数（先例 category-chart.ts 的数据形态纯函数）。tooltip「金额 · 占比%」
// 复用 category-chart 的收口纯函数 barTooltipLabel，分母 = 后端载荷的全量合计。
import type { MerchantShare } from '@/types'

/** 名次梯度色（同色系蓝，与柱图主色板同族）：固定色相/饱和度，亮度随名次单调上升 */
const RANK_HUE = 217
const RANK_SAT = 52
const RANK_L_DARK = 30
const RANK_L_LIGHT = 72

/** 横向柱状图单根柱：后端返回序即柱序 */
export interface MerchantBar {
  /** 商户 id（下钻跳转载荷用，issue #589）：后端 MerchantShare.merchant_id 同源，
   *  含软删商户的历史名下钻照常（TransactionFilter 既有口径）。 */
  merchant_id: string
  name: string
  /** 净支出（分）：负值（退款大于支出）与 0 如实渲染，口径归后端 */
  value: number
  color: string
}

/**
 * 名次梯度色：rank 1（index 0）最深、末位最浅；单柱（count=1）取最深档。
 * 纯函数便于单测单调性（视图只消费，不再手搓颜色数学）。
 */
export function merchantRankColor(index: number, count: number): string {
  const t = count <= 1 ? 0 : index / (count - 1)
  const l = RANK_L_DARK + t * (RANK_L_LIGHT - RANK_L_DARK)
  return `hsl(${RANK_HUE}, ${RANK_SAT}%, ${l.toFixed(1)}%)`
}

/** 图行构建：后端返回序一一映射（不重排不过滤），色随名次梯度；
 *  merchant_id 从 MerchantShare 透传（下钻跳转载荷用，issue #589）。 */
export function merchantBars(rows: MerchantShare[]): MerchantBar[] {
  return rows.map((r, i) => ({
    merchant_id: r.merchant_id,
    name: r.merchant_name,
    value: r.amount_cents,
    color: merchantRankColor(i, rows.length),
  }))
}
