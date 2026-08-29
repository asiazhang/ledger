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
 *  icon/color 来自商户字典行（软删商户的历史引用照常统计显示）。 */
export interface MerchantShare {
  merchant_id: string
  merchant_name: string
  icon: string | null
  color: string | null
  amount_cents: number
}
