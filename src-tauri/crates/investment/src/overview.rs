//! 投资概览（InvestmentOverview，spec #1532 / issue #1536）：投资页「概览」页签的
//! 全页折本位币单值读数单点。
//!
//! 口径（词汇表「投资概览」/「可投资资产（InvestableAssets）」）：
//! - **全页折全局默认币种（DefaultCurrency）单值**：本页所有金额折算到同一币种后
//!   相加，与持仓视图「按账户币种分组、不跨币种合并」的分工见 ADR-0130；缺折算
//!   汇率时错误上抛（码化 `fx.rate-missing`），不静默混币种、不给半截数字。
//! - **可投资资产 = 投资账户现金腿 + 持仓市值腿**：两腿各有单点（财务自由度
//!   [`super::financial_freedom`] 模块），本函数只做相加与投影，不复制口径表达式。
//! - **缺价持仓跳过但显式可见**：从未录价（或缺价格币→账户币汇率）的持仓按空值
//!   语义跳过、不以零计入，同时给出未计入的持仓数——既不虚增也不静默低估。
//! - **排除隐藏账户**（含黑洞，见 AI 导入域）：隐藏投资账户的现金与持仓一并不计入，
//!   未计入计数同面排除；生活现金（非投资账户）不进本口径。
//! - **有无投资账户照常出数**：没有投资账户时合计为 0，`has_investment_account`
//!   供展示层给引导句——不隐藏功能。
//! - 实时只读聚合，不落库、无写入路径（ADR-0013）。

use rusqlite::Connection;

use ledger_accounts::AccountType;
use ledger_accounts::balance::list_accounts_with_visibility;
use ledger_infra::error::Result;
use ledger_transaction::amount;

use super::financial_freedom::{
    HOLDINGS_VISIBLE_FACET, query_investable_assets_cash_leg_cents,
    query_investable_assets_holdings_leg_cents,
};
use super::model::InvestmentOverview;

/// conn 级聚合：投资概览读数（只读）。
pub fn query_investment_overview(conn: &Connection) -> Result<InvestmentOverview> {
    // 两腿各自单点取值（隐藏账户排除、缺价跳过、缺汇率上抛都在腿内），合计在此相加。
    let investment_cash_cents = query_investable_assets_cash_leg_cents(conn)?;
    let holdings_market_value_cents = query_investable_assets_holdings_leg_cents(conn)?;

    Ok(InvestmentOverview {
        native_currency: amount::default_currency_code(conn)?,
        investable_assets_cents: investment_cash_cents + holdings_market_value_cents,
        investment_cash_cents,
        holdings_market_value_cents,
        missing_price_holding_count: count_unpriced_holdings(conn)?,
        has_investment_account: has_investment_account(conn)?,
    })
}

/// 未计入的持仓数（`v_holdings` 市值为 NULL 的行）：缺现价或缺价格币→账户币汇率
/// 的持仓——与持仓市值腿共用同一取数面（[`HOLDINGS_VISIBLE_FACET`]），故计数与
/// 合计不会漂移。
fn count_unpriced_holdings(conn: &Connection) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) {HOLDINGS_VISIBLE_FACET} AND h.market_value_cents IS NULL");
    let count = conn.query_row(&sql, [], |row| row.get(0))?;
    Ok(count)
}

/// 账本内是否存在未删除的投资账户（含隐藏）：判定「还没有投资账户」的引导句，
/// 不参与任何金额口径——只回答「用户建过投资账户没有」。走账户域读投影
/// （软删排除）与 [`AccountType`] 闭集，本域不另写账户表的 kind 字面量。
fn has_investment_account(conn: &Connection) -> Result<bool> {
    Ok(list_accounts_with_visibility(conn, true)?
        .iter()
        .any(|account| account.kind == AccountType::Investment))
}
