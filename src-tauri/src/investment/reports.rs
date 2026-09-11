use rusqlite::Connection;

use super::model::{
    AccountPnl, CurrencyCumulativePnl, CurrencyPnl, InstrumentPnl, PnlFilter, RealizedPnlSummary,
    YearPnl,
};
use crate::db::query::query_all;
use crate::error::Result;

/// 按币种分组的累计收益（issue #1077 / 词汇表「累计收益（CumulativePnl）」）：
/// 未实现盈亏（Holding 侧，`v_holdings`）+ 已实现盈亏（RealizedPnl 侧，
/// `security_lot_sales`）两腿相加，按币种独立成组、不做跨币种折算。
///
/// **复用两条既有读口径、不改其定义**：未实现腿直接取 `v_holdings` 的账户本位币
/// `unrealized_pnl_cents`（账户币种经 `accounts` 取出，与视图折算口径同源）；已实现腿
/// 与 [`query_realized_pnl_summary`] 同一过滤——软删账户（`a.is_deleted=0`）与软删交易
/// （`t.is_deleted=0`）排除，隐藏账户照常计入。
///
/// **空值语义采 Holding 侧**：缺价 / 缺汇率持仓的未实现腿为 NULL，在 `SUM` 中跳过、
/// 不以零计入（与持仓视图合计的既有空值语义一致）；已实现腿为平仓匹配、无此空值。
/// 某币种两腿皆空时不出现该分组。
pub fn query_cumulative_pnl_summary(conn: &Connection) -> Result<Vec<CurrencyCumulativePnl>> {
    // 两腿 `UNION ALL` 后按币种分组求和：未实现腿按账户币种、已实现腿按匹配行币种，
    // 两条口径的币种在单账户内同源（buy/sell 记录币种恒为账户币），故同组可直接相加。
    let sql = "SELECT currency_code, SUM(amount_cents) FROM (\
                   SELECT a.currency_code AS currency_code, v.unrealized_pnl_cents AS amount_cents \
                   FROM v_holdings v \
                   JOIN accounts a ON a.id = v.account_id \
                   WHERE v.unrealized_pnl_cents IS NOT NULL \
                   UNION ALL \
                   SELECT sls.currency_code AS currency_code, sls.realized_pnl_cents AS amount_cents \
                   FROM security_lot_sales sls \
                   JOIN transactions t ON t.id = sls.sell_transaction_id \
                   JOIN accounts a ON a.id = t.account_id AND a.is_deleted = 0 \
                   WHERE t.is_deleted = 0 \
               ) GROUP BY currency_code ORDER BY currency_code";
    query_all(conn, sql, [])
}

pub fn query_realized_pnl_summary(
    conn: &Connection,
    filter: &PnlFilter,
) -> Result<RealizedPnlSummary> {
    // 账户软删在 JOIN 条件排除（issue #217 定案「删除账户 = 从全部投资视角消失」，
    // 与 v_holdings / 时点持仓读口径对齐）；交易行软删（t.is_deleted，含 sell 删除
    // 不清理匹配行的既有行为，ADR-0013）同样排除——隐藏账户不是软删除，照常计入。
    let base_from = "FROM security_lot_sales sls \
                     JOIN transactions t ON t.id = sls.sell_transaction_id \
                     JOIN security_transactions st ON st.transaction_id = sls.sell_transaction_id \
                     JOIN instruments i ON i.id = st.instrument_id \
                     JOIN accounts a ON a.id = t.account_id AND a.is_deleted = 0";

    let mut conditions: Vec<String> = vec!["t.is_deleted=0".to_string()];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(acct_id) = &filter.account_id {
        params.push(Box::new(acct_id.clone()));
        conditions.push(format!("t.account_id=?{}", params.len()));
    }
    if let Some(inst_id) = &filter.instrument_id {
        params.push(Box::new(inst_id.clone()));
        conditions.push(format!("st.instrument_id=?{}", params.len()));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    // 汇总按匹配行币种分组（ADR-0107 决策 6）：不做跨币种折算，各币种小计独立成立——
    // 原「各币种裸数字直接 SUM」的混算口径废止（多币种账户下合计是错的）。
    // 逐匹配「卖出明细」查询已随明细卡退役（决策 1），本函数只产出三张汇总视图 + 分组总数。
    let total_sql = format!(
        "SELECT sls.currency_code, COALESCE(SUM(sls.realized_pnl_cents), 0) \
         {base_from}{where_clause} GROUP BY sls.currency_code ORDER BY sls.currency_code"
    );
    let year_sql = format!(
        "SELECT substr(t.date, 1, 4) AS year, sls.currency_code, SUM(sls.realized_pnl_cents) \
         {base_from}{where_clause} GROUP BY year, sls.currency_code ORDER BY year, sls.currency_code"
    );
    let account_sql = format!(
        "SELECT a.id, a.name, sls.currency_code, COALESCE(SUM(sls.realized_pnl_cents), 0) \
         {base_from}{where_clause} GROUP BY a.id, sls.currency_code ORDER BY a.name, sls.currency_code"
    );
    let instrument_sql = format!(
        "SELECT i.id, i.symbol, i.name, sls.currency_code, COALESCE(SUM(sls.realized_pnl_cents), 0) \
         {base_from}{where_clause} GROUP BY i.id, sls.currency_code ORDER BY i.symbol, sls.currency_code"
    );

    let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();

    let total: Vec<CurrencyPnl> = query_all(conn, &total_sql, params_ref.as_slice())?;
    let by_year: Vec<YearPnl> = query_all(conn, &year_sql, params_ref.as_slice())?;
    let by_account: Vec<AccountPnl> = query_all(conn, &account_sql, params_ref.as_slice())?;
    let by_instrument: Vec<InstrumentPnl> =
        query_all(conn, &instrument_sql, params_ref.as_slice())?;

    Ok(RealizedPnlSummary {
        total,
        by_year,
        by_account,
        by_instrument,
    })
}
