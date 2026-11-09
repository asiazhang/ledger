//! 投资概览（InvestmentOverview，spec #1532 / issue #1536、#1537）：投资页「概览」页签的
//! 全页折本位币单值读数单点。
//!
//! 口径（词汇表「投资概览」/「可投资资产（InvestableAssets）」/「累计收益
//! （CumulativePnl）」）：
//! - **全页折全局默认币种（DefaultCurrency）单值**：本页所有金额折算到同一币种后
//!   相加，与持仓视图「按账户币种分组、不跨币种合并」的分工见 ADR-0131；缺折算
//!   汇率时错误上抛（码化 `fx.rate-missing`），不静默混币种、不给半截数字。
//! - **可投资资产 = 投资账户现金腿 + 持仓市值腿**：两腿各有单点（财务自由度
//!   [`super::financial_freedom`] 模块），本函数只做相加与投影，不复制口径表达式。
//! - **投资合计三项（#1537）**：总市值 = Σ 折本位币持仓市值（与持仓市值腿同一
//!   聚合，字段各存其名）；持仓收益（未实现盈亏）= Σ 折本位币未实现盈亏；
//!   累计收益 = 持仓收益 + 已实现盈亏（卖出匹配）+ 累计分红（dividend 现金腿）
//!   ——后两腿与既有分组口径（[`super::reports`]）同源同过滤，唯逐行折本位币，
//!   与既有「按币种分组、不跨币种折算」的分工见 ADR-0131 决策 3。
//! - **缺价持仓跳过但显式可见**：从未录价（或缺价格币→账户币汇率）的持仓按空值
//!   语义跳过、不以零计入，同时给出未计入的持仓数——既不虚增也不静默低估。
//! - **排除隐藏账户**（含黑洞，见 AI 导入域，页面级边界「同 InvestableAssets
//!   口径」）：隐藏投资账户的现金、持仓、已实现与分红一并不计入，未计入计数
//!   同面排除；生活现金（非投资账户）不进本口径。
//! - **有无投资账户照常出数**：没有投资账户时合计为 0，`has_investment_account`
//!   供展示层给引导句——不隐藏功能。
//! - 实时只读聚合，不落库、无写入路径（ADR-0013）。

use rusqlite::Connection;

use ledger_accounts::AccountType;
use ledger_accounts::balance::list_accounts_with_visibility;
use ledger_infra::db::query::query_all;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;
use ledger_transaction::amount;

use super::reports::LegEntry;

use super::financial_freedom::{
    HOLDINGS_VISIBLE_FACET, query_investable_assets_cash_leg_cents,
    query_investable_assets_holdings_leg_cents, sum_holdings_facet_column_cents,
};
use super::model::InvestmentOverview;

/// conn 级聚合：投资概览读数（只读）。
///
/// **读快照一致性（issue #1699）**：现金 / 持仓 / 未实现 / 已实现 / 分红多腿与
/// 计数、引导判定多段取数，整体收进同一读事务（嵌套感知）——写提交落在语句
/// 之间会让同屏单值的各腿不同时点。
pub fn query_investment_overview(conn: &Connection) -> Result<InvestmentOverview> {
    ensure_transaction(conn, || {
        // 两腿各自单点取值（隐藏账户排除、缺价跳过、缺汇率上抛都在腿内），合计在此相加。
        let investment_cash_cents = query_investable_assets_cash_leg_cents(conn)?;
        let holdings_market_value_cents = query_investable_assets_holdings_leg_cents(conn)?;
        // 投资合计三项（#1537）：未实现腿与持仓市值腿同一取数面，单次取值两处消费。
        let unrealized_pnl_cents = sum_holdings_facet_column_cents(conn, "unrealized_pnl_cents")?;
        let realized_pnl_cents = query_realized_pnl_cents(conn)?;
        let dividend_total_cents = query_dividend_total_cents(conn)?;

        Ok(InvestmentOverview {
            native_currency: amount::default_currency_code(conn)?,
            investable_assets_cents: investment_cash_cents + holdings_market_value_cents,
            investment_cash_cents,
            // 总市值与持仓市值腿在全页折本位币口径下是同一聚合（同取数面同折算）：
            // 展示面「可投资资产拆分」与「投资合计」各答一次问，字段各存其名。
            total_market_value_cents: holdings_market_value_cents,
            holdings_market_value_cents,
            unrealized_pnl_cents,
            // 累计收益三腿相加（词汇表「累计收益」），不是第三种成本法。
            cumulative_pnl_cents: unrealized_pnl_cents + realized_pnl_cents + dividend_total_cents,
            missing_price_holding_count: count_unpriced_holdings(conn)?,
            has_investment_account: has_investment_account(conn)?,
        })
    })
}

/// 未计入的持仓数（`v_holdings` 市值为 NULL 的行）：缺现价或缺价格币→账户币汇率
/// 的持仓——与持仓市值腿共用同一取数面（[`HOLDINGS_VISIBLE_FACET`]，经
/// `financial_freedom` 模块单点），故计数与合计不会漂移。
fn count_unpriced_holdings(conn: &Connection) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) {HOLDINGS_VISIBLE_FACET} AND h.market_value_cents IS NULL");
    let count = conn.query_row(&sql, [], |row| row.get(0))?;
    Ok(count)
}

/// 逐行折全局默认币种后求和（缺汇率码化上抛，不给半截数字）。
fn sum_leg_entries(conn: &Connection, sql: &str) -> Result<i64> {
    let entries: Vec<LegEntry> = query_all(conn, sql, [])?;
    let mut sum = 0i64;
    for entry in entries {
        sum += amount::convert_to_native_current(conn, entry.amount_cents, &entry.currency_code)?;
    }
    Ok(sum)
}

/// conn 级聚合：投资合计·**已实现盈亏腿**（折本位币，分）。
///
/// 卖出匹配盈亏（`security_lot_sales`），与既有分组口径
/// （[`super::reports::query_cumulative_pnl_summary`] 已实现腿）同一过滤：软删
/// 账户与软删交易排除（隐藏账户在既有分组口径照常计入，本页按页面级边界
/// 「同 InvestableAssets 口径」收紧排除，差异轴见 ADR-0131 决策 3）。逐行折
/// 全局默认币种，缺汇率码化上抛。
fn query_realized_pnl_cents(conn: &Connection) -> Result<i64> {
    sum_leg_entries(
        conn,
        "SELECT sls.realized_pnl_cents, sls.currency_code \
         FROM security_lot_sales sls \
         JOIN transactions t ON t.id = sls.sell_transaction_id \
         JOIN accounts a ON a.id = t.account_id AND a.is_deleted=0 AND a.is_hidden=0 \
         WHERE t.is_deleted=0",
    )
}

/// conn 级聚合：投资合计·**累计分红腿**（折本位币，分）。
///
/// dividend 现金腿（kind 与扩展行双条件，ADR-0109），与既有分组口径同源同过滤
/// （软删账户与软删流水排除、隐藏账户按本页口径收紧排除）。分红不摊薄成本、
/// 只按自身现金流量入累计收益；币种 = 交易行币种（写路径守卫保证 = 到账账户
/// 币种），逐行折全局默认币种，缺汇率码化上抛。
fn query_dividend_total_cents(conn: &Connection) -> Result<i64> {
    sum_leg_entries(
        conn,
        "SELECT t.amount_cents, t.currency_code \
         FROM transactions t \
         JOIN security_transactions st ON st.transaction_id = t.id AND st.action='dividend' \
         JOIN accounts a ON a.id = t.account_id AND a.is_deleted=0 AND a.is_hidden=0 \
         WHERE t.is_deleted=0 AND t.kind='dividend'",
    )
}

/// 账本内是否存在未删除的投资账户（含隐藏）：判定「还没有投资账户」的引导句，
/// 不参与任何金额口径——只回答「用户建过投资账户没有」。走账户域读投影
/// （软删排除）与 [`AccountType`] 闭集，本域不另写账户表的 kind 字面量。
fn has_investment_account(conn: &Connection) -> Result<bool> {
    Ok(list_accounts_with_visibility(conn, true)?
        .iter()
        .any(|account| account.kind == AccountType::Investment))
}
