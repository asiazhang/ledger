// 商户消费排行表格行构建（issue #618 表格化）：排序与 topN 截断已收口后端
// `merchant_shares`，前端按返回序渲染、零口径逻辑。行色与支出分类构成同源：
// 分类色板按名次序取色（多颜色，第 1 名 = 色板首位深蓝），收 category-chart 的
// paletteColor 纯函数单一来源。占比% 收 category-chart 的 sharePercent 单一来源，
// 分母 = 后端载荷的全量合计（与 topN 截断无关，沿用既有 tooltip 同一取整口径）。
import type { MerchantShare } from '@/types'
import { paletteColor, sharePercent } from '@/utils/category-chart'

/** 表格单行：后端返回序即行序。列为 商户名 | 金额内嵌条 | 金额数字 | 占比% | 笔数。 */
export interface MerchantTableRow {
  /** 商户 id（下钻跳转载荷用，issue #589）：后端 MerchantShare.merchant_id 同源，
   *  含软删商户的历史名下钻照常（TransactionFilter 既有口径）。 */
  merchant_id: string
  name: string
  /** 净支出（分）：负值（退款大于支出）与 0 如实展示，口径归后端 */
  amount_cents: number
  /** 内嵌条宽度百分数（0..100，两位小数内）：= 金额 ÷ 显示区最大正金额（topN 下
   *  即第一名）× 100；负净额与 0 不画条（归 0），全负 / 全零行集除零防护同归 0；
   *  两位小数内收口，浮点尾数不进 DOM（如 19.800000000000004%）。 */
  barPct: number
  /** 占比%（整数）：分母 = 后端载荷全量合计（sharePercent 单一来源），负值照实 */
  sharePct: number
  /** 交易笔数（报表口径）：MerchantShare.transaction_count 同源透传 */
  transactionCount: number
  color: string
}

/** 表格行构建：后端返回序一一映射（不重排不过滤），色随名次取分类色板；
 *  条长比例与占比口径在此收口，组件零口径逻辑。`totalCents` = 后端载荷全量合计。 */
export function merchantTableRows(
  rows: MerchantShare[],
  totalCents: number,
): MerchantTableRow[] {
  const maxPositive = rows.reduce((m, r) => Math.max(m, r.amount_cents), 0)
  return rows.map((r, i) => ({
    merchant_id: r.merchant_id,
    name: r.merchant_name,
    amount_cents: r.amount_cents,
    barPct:
      maxPositive > 0
        ? Math.round((Math.max(r.amount_cents, 0) / maxPositive) * 10000) / 100
        : 0,
    sharePct: sharePercent(r.amount_cents, totalCents),
    transactionCount: r.transaction_count,
    color: paletteColor(i),
  }))
}
