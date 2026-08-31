//! 财务自由度领域模型：自由度总览（issue #343 / ADR-0048）。

use serde::Serialize;

/// `financial_freedom` 命令返回的财务自由度总览（本位币口径，金额单位：分）。
///
/// 自由度 = 可投资资产 × 3% 安全提取率 ÷ 年度预算总额 × 100%（口径取舍见
/// docs/adr/0048-financial-freedom-ratio.md）。实时计算不落库；未设预算时
/// 分母为零、ratio 与 coverage_years 均为 0（占位引导在展示层，不回退实际支出）。
#[derive(Debug, Clone, Serialize)]
pub struct FinancialFreedomOverview {
    /// 自由度百分比（一位小数）
    pub ratio: f64,
    /// 分子：可投资资产合计（本位币，分）= Σ 折本位币持仓市值 + Σ 折本位币投资账户余额
    /// （排除隐藏账户；未录价持仓按空值语义不计入）
    pub numerator_cents: i64,
    /// 分母：年度预算总额（分）= Σ 月度预算 × 12 + Σ 年度预算（全部未删除，无窗口不滚动）
    pub denominator_cents: i64,
    /// 覆盖年数（一位小数）= 分子 ÷ 分母（零分母为 0）
    pub coverage_years: f64,
    /// 折算基准币种（全局默认币种）
    pub native_currency: String,
}
