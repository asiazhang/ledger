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
 *  商户名取自字典行现名（软删商户的历史引用照常统计显示）；icon/color 已退役（issue #223）。 */
export interface MerchantShare {
  merchant_id: string
  merchant_name: string
  amount_cents: number
}
