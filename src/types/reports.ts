/** 报表日期极值范围（issue #266 / #389）：数据驱动的极值日期对，空库双 null。 */
export interface ReportDateRange {
  min_date: string | null
  max_date: string | null
}

/** 报表期间过滤（issue #411 / ADR-0057）：YYYY-MM-DD 含边界闭区间。
 *  三张报表命令的现役 from/to 参数形状；遗留参数（月度汇总/商户排行的 year、
 *  分类份额的 month+year）已发布冻结保留，前端不再使用。 */
export interface ReportPeriodRange {
  from: string
  to: string
}

export interface MonthlySummary {
  month: string
  income_cents: number
  expense_cents: number
  refund_cents: number
}

export interface CategoryShare {
  category_id: string
  category_name: string
  amount_cents: number
}

/** 商户消费排行行（issue #192）：expense_net（毛支出 − 退款）按商户聚合、本位币口径；
 *  商户名取自字典行现名（软删商户的历史引用照常统计显示）；icon/color 已退役（issue #223）。
 *  transaction_count（issue #617）：该商户在期间内、参与排行口径（支出 + 退款）的
 *  交易记录数，与金额同口径——退款笔数计入、无商户交易不进排行不计数、
 *  软删商户历史引用照常计数。区别于核心域 Merchant「关联交易条数（毛笔数）」。 */
export interface MerchantShare {
  merchant_id: string
  merchant_name: string
  amount_cents: number
  transaction_count: number
}

/** 商户消费排行载荷（issue #588）：rows = 排行行（后端已排序与 topN 截断，
 *  前端按返回序渲染）；total_cents = 本期全部商户净支出合计，是 tooltip 占比的
 *  分母（与 top_n 截断无关）。载荷形状为内部 IPC 契约，前后端同发更新。 */
export interface MerchantSharesReport {
  rows: MerchantShare[]
  total_cents: number
}
