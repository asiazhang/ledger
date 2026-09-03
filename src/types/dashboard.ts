/**
 * 首页财务全貌领域类型（issue #142）。
 */

/** `dashboard_overview` 命令返回的净资产总览（本位币口径，金额单位：分）。 */
export interface DashboardOverview {
  /** 折算基准币种（全局默认币种） */
  native_currency: string
  /** 净资产合计（分）= 非投资账户余额合计 + 持仓市值合计 + 在持实物资产估值合计 */
  net_worth_cents: number
  /** 非投资账户折本位币余额合计（分；投资账户与隐藏/黑洞账户不计入） */
  accounts_balance_cents: number
  /** 折本位币持仓市值合计（分；未录价标的按空值语义不计入） */
  holdings_market_value_cents: number
  /** 在持实物资产估值折本位币合计（分；已处置 / 软删不计入，缺汇率报错上抛） */
  physical_assets_value_cents: number
}
