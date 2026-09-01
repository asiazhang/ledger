/**
 * 财务自由度领域类型（issue #343 / ADR-0048）。
 */

/** `financial_freedom` 命令返回的财务自由度总览（本位币口径，金额单位：分）。 */
export interface FinancialFreedomOverview {
  /** 自由度百分比（一位小数）= 可投资资产 × 3% 安全提取率 ÷ 年度预算总额 × 100% */
  ratio: number
  /** 分子：可投资资产合计（分）= Σ 折本位币持仓市值 + Σ 折本位币投资账户余额（排除隐藏账户） */
  numerator_cents: number
  /** 分母：年度预算总额（分）= Σ 月度预算 × 12 + Σ 年度预算（全部未删除，无窗口不滚动） */
  denominator_cents: number
  /** 覆盖年数（一位小数）= 分子 ÷ 分母；零分母为 0 */
  coverage_years: number
  /** 折算基准币种（全局默认币种） */
  native_currency: string
}
