// 商户消费排行横向柱状图的数据形态（issue #588）：排序与 topN 截断已收口后端
// `merchant_shares`，前端按返回序渲染、零口径逻辑。柱色与支出分类构成同源：
// 分类色板按名次序取色（多颜色，第 1 名 = 色板首位深蓝），收 category-chart 的
// paletteColor 纯函数单一来源；色板为 hex 实色，柱体「基线淡出 → 柱端实色」的
// 渐变由 softBarFillPlugin 绘制期换装（先例分类图，无需本模块参与颜色数学）。
// tooltip「金额 · 占比%」复用 category-chart 的收口纯函数 barTooltipLabel，
// 分母 = 后端载荷的全量合计。
import type { MerchantShare } from '@/types'
import { paletteColor } from '@/utils/category-chart'

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

/** 图行构建：后端返回序一一映射（不重排不过滤），色随名次取分类色板；
 *  merchant_id 从 MerchantShare 透传（下钻跳转载荷用，issue #589）。 */
export function merchantBars(rows: MerchantShare[]): MerchantBar[] {
  return rows.map((r, i) => ({
    merchant_id: r.merchant_id,
    name: r.merchant_name,
    value: r.amount_cents,
    color: paletteColor(i),
  }))
}
