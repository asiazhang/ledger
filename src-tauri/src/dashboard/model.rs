//! 首页财务全貌领域模型（#421 随域归位）：净资产总览（issue #142）。
//!
//! 自全局模型目录迁入本域（#417 归属原则），域内类型经 `dashboard`
//! 逐类型再导出，消费方经域路径显式 import，禁止 glob。

use serde::Serialize;

/// `dashboard_overview` 命令返回的净资产总览（本位币口径，金额单位：分）。
///
/// 净资产 = Σ 非投资账户折本位币余额 + Σ 折本位币持仓市值（真实财富视角，
/// 口径取舍见 docs/adr/0020-net-worth-real-wealth-perspective.md）。
/// 从未录价的持仓市值按空值语义跳过，不以零计入。
#[derive(Debug, Clone, Serialize)]
pub struct DashboardOverview {
    /// 折算基准币种（全局默认币种）
    pub native_currency: String,
    /// 净资产合计（本位币，分）
    pub net_worth_cents: i64,
    /// 非投资账户折本位币余额合计（分；投资账户与隐藏/黑洞账户不计入）
    pub accounts_balance_cents: i64,
    /// 折本位币持仓市值合计（分；未录价标的按空值语义不计入）
    pub holdings_market_value_cents: i64,
}
