//! 投资盈亏读投影（issue #1077 / #1078；ADR-0107 / ADR-0129 / ADR-0114）：
//! 已实现盈亏汇总与按币种累计收益查询的只读单点。
//!
//! - [`query_cumulative_pnl_summary`]：未实现盈亏（`v_holdings`）+ 已实现盈亏（卖出
//!   匹配）+ 累计分红三腿按币种独立成组，不跨币种折算；缺价 / 缺汇率持仓按空值跳过。
//! - [`query_cumulative_pnl_native_total`]：同三腿的折本位币单值（issue #1797，
//!   持仓页签合计卡与首页投资概览卡消费面）。
//! - [`query_realized_pnl_summary`]：盈亏页按年 / 按账户两表（已实现 + 分红两腿）
//!   与按币种总数、按标的行；软删账户与软删流水排除、隐藏账户照常计入。
//! - [`query_holdings_summary_by_currency`]：持仓市值 / 未实现盈亏按账户币种分组合计。

use rusqlite::Connection;

use ledger_transaction::amount;

use super::model::{
    AccountPnl, CumulativePnlNativeTotal, CurrencyCumulativePnl, CurrencyHoldingTotals,
    CurrencyPnl, InstrumentPnl, PnlFilter, RealizedPnlSummary, YearPnl,
};
use ledger_infra::db::query::{FromRow, query_all};
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;

/// 已实现/分红腿行：金额（分，非空）+ 币种（逐行折本位币消费面的行载体）。
pub(crate) struct LegEntry {
    pub amount_cents: i64,
    pub currency_code: String,
}

impl FromRow for LegEntry {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(LegEntry {
            amount_cents: row.get(0)?,
            currency_code: row.get(1)?,
        })
    }
}

/// 累计收益三腿的 UNION ALL 片段（分组与折本位币单值两聚合共用，口径表达式
/// 不复制）：未实现腿按账户币种（缺价 / 缺汇率持仓的 NULL 行直接不出——分组
/// 靠 SUM 跳空、单值靠逐行 Option 过滤，两侧空值语义同源）；已实现腿按匹配
/// 行币种、分红腿按交易行币种；过滤同源（软删账户与软删交易排除、隐藏账户
/// 照常计入，issue #217 定案）。
const CUMULATIVE_PNL_LEGS: &str = "SELECT a.currency_code AS currency_code, v.unrealized_pnl_cents AS amount_cents \
     FROM v_holdings v \
     JOIN accounts a ON a.id = v.account_id \
     WHERE v.unrealized_pnl_cents IS NOT NULL \
     UNION ALL \
     SELECT sls.currency_code AS currency_code, sls.realized_pnl_cents AS amount_cents \
     FROM security_lot_sales sls \
     JOIN transactions t ON t.id = sls.sell_transaction_id \
     JOIN accounts a ON a.id = t.account_id AND a.is_deleted = 0 \
     WHERE t.is_deleted = 0 \
     UNION ALL \
     SELECT t.currency_code AS currency_code, t.amount_cents AS amount_cents \
     FROM transactions t \
     JOIN security_transactions st ON st.transaction_id = t.id AND st.action='dividend' \
     JOIN accounts a ON a.id = t.account_id AND a.is_deleted = 0 \
     WHERE t.is_deleted = 0 AND t.kind='dividend'";

/// 按币种分组的累计收益（issue #1077 / #1078 / 词汇表「累计收益（CumulativePnl）」）：
/// 未实现盈亏（Holding 侧，`v_holdings`）+ 已实现盈亏（RealizedPnl 侧，
/// `security_lot_sales`）+ 累计分红（`security_transactions` 的 dividend 行）
/// 三腿相加，按币种独立成组、不做跨币种折算。
///
/// **复用既有读口径、不改其定义**：未实现腿直接取 `v_holdings` 的账户本位币
/// `unrealized_pnl_cents`（账户币种经 `accounts` 取出，与视图折算口径同源）；已实现腿
/// 与 [`query_realized_pnl_summary`] 同一过滤——软删账户（`a.is_deleted=0`）与软删交易
/// （`t.is_deleted=0`）排除，隐藏账户照常计入；分红腿同一过滤（软删账户与软删分红
/// 流水排除、隐藏账户照常计入），币种取交易行币种（写路径守卫保证 = 到账账户币种）。
///
/// **空值语义采 Holding 侧**：缺价 / 缺汇率持仓的未实现腿为 NULL，在 `SUM` 中跳过、
/// 不以零计入（与持仓视图合计的既有空值语义一致）；已实现腿为平仓匹配、无此空值。
/// 某币种三腿皆空时不出现该分组。
///
/// **分红是第三腿、不摊薄成本**（issue #1078 / ADR-0109）：分红不触碰 FIFO 批次
/// （未实现盈亏与已实现盈亏的既有口径逐位不变），只按其自身现金流量入累计收益——
/// 与「当前市值 + 累计卖出净收入 − 累计买入总支出 + 累计分红」的现金流口径一致。
pub fn query_cumulative_pnl_summary(conn: &Connection) -> Result<Vec<CurrencyCumulativePnl>> {
    // 三腿 `UNION ALL`（[`CUMULATIVE_PNL_LEGS`]）后按币种分组求和：未实现腿按账户
    // 币种、已实现腿按匹配行币种、分红腿按交易行币种，三条口径的币种在单账户内
    // 同源（buy/sell 与 dividend 记录币种恒为账户币），故同组可直接相加。
    let sql = format!(
        "SELECT currency_code, SUM(amount_cents) FROM ({CUMULATIVE_PNL_LEGS}) \
         GROUP BY currency_code ORDER BY currency_code"
    );
    query_all(conn, &sql, [])
}

/// 累计收益·折本位币单值（issue #1797，[`CumulativePnlNativeTotal`]）：同
/// [`query_cumulative_pnl_summary`] 的三腿取数（[`CUMULATIVE_PNL_LEGS`]，同过滤同
/// 空值语义），唯逐行按当期汇率折全局默认币种后求和（折算单点在交易域入口；
/// 缺汇率码化上抛 `fx.rate-missing`，不静默给半截数字——与投资概览页签同款硬
/// 形态，展示层整卡警告 + 重试）。
///
/// **读快照一致性（issue #1699 同款）**：三腿取数与逐行折算多语句，整体收进
/// 同一读事务（嵌套感知）——写提交落在语句之间会合计各腿不同时点。
pub fn query_cumulative_pnl_native_total(conn: &Connection) -> Result<CumulativePnlNativeTotal> {
    ensure_transaction(conn, || {
        let sql = format!("SELECT amount_cents, currency_code FROM ({CUMULATIVE_PNL_LEGS})");
        let entries: Vec<LegEntry> = query_all(conn, &sql, [])?;
        let mut total = 0i64;
        for entry in entries {
            total +=
                amount::convert_to_native_current(conn, entry.amount_cents, &entry.currency_code)?;
        }
        Ok(CumulativePnlNativeTotal {
            total_cents: total,
            native_currency: amount::default_currency_code(conn)?,
        })
    })
}

/// 按币种分组的持仓合计（issue #1196 / ADR-0114 跨账本汇总的域读投影）：
/// 持仓页签合计既有口径的后端读函数——`v_holdings` 市值/未实现盈亏两列按账户
/// 币种分组求和（issue #902 形态先例，同 ADR-0107 决策 6 口径），不跨币种折算。
/// 软删账户由视图内建排除；隐藏账户照常计入（Holding 口径）；空值跳过、不以零
/// 计入，两列皆空的币种组不出现（同 [`query_cumulative_pnl_summary`] 的空组语义）。
pub fn query_holdings_summary_by_currency(conn: &Connection) -> Result<Vec<CurrencyHoldingTotals>> {
    let sql = "SELECT currency_code, SUM(market_value_cents), SUM(unrealized_pnl_cents) FROM (\
                   SELECT a.currency_code AS currency_code, \
                          v.market_value_cents, v.unrealized_pnl_cents \
                   FROM v_holdings v \
                   JOIN accounts a ON a.id = v.account_id \
               ) \
               GROUP BY currency_code \
               HAVING SUM(market_value_cents) IS NOT NULL \
                   OR SUM(unrealized_pnl_cents) IS NOT NULL \
               ORDER BY currency_code";
    query_all(conn, sql, [])
}

/// 盈亏页读投影（ADR-0107 / ADR-0129）：按年与按账户两张表各带两条腿——已实现盈亏
/// （卖出匹配）与现金分红（dividend 行），并给出域内相加的合计（词汇表「已实现收益
/// （RealizedGain）」）。按币种分组、不跨币种折算（ADR-0107 决策 6）；分红腿与已实现腿
/// 同源同过滤（ADR-0129 决策 3）：账户 / 标的筛选对两腿各用一次、软删账户与软删流水
/// 排除、隐藏账户照常计入。按币种总数（`total`）与按标的行维持已实现盈亏专义，不扩分红
/// （ADR-0129 决策 4）。
///
/// **读快照一致性（issue #1699）**：total / by_year / by_account / by_instrument
/// 四查整体收进同一读事务（嵌套感知）——写提交落在语句之间会总量≠分量和，
/// 同屏口径自相矛盾。
pub fn query_realized_pnl_summary(
    conn: &Connection,
    filter: &PnlFilter,
) -> Result<RealizedPnlSummary> {
    ensure_transaction(conn, || {
        // 账户软删在 JOIN 条件排除（issue #217 定案「删除账户 = 从全部投资视角消失」，
        // 与 v_holdings / 时点持仓读口径对齐）；交易行软删（t.is_deleted）同样排除——
        // sell 删除自 ADR-0097 起回补持仓并清空其匹配行，此处的读口径排除保留以覆盖
        // 旧版本遗留的幽灵匹配（issue #940）；隐藏账户不是软删除，照常计入。
        let base_from = "FROM security_lot_sales sls \
                     JOIN transactions t ON t.id = sls.sell_transaction_id \
                     JOIN security_transactions st ON st.transaction_id = sls.sell_transaction_id \
                     JOIN instruments i ON i.id = st.instrument_id \
                     JOIN accounts a ON a.id = t.account_id AND a.is_deleted = 0";
        // 分红腿的取数面（ADR-0109：kind 与扩展行双条件同 [`query_cumulative_pnl_summary`]）；
        // 不 JOIN instruments——标的过滤直接落在扩展行的 instrument_id 上，与已实现腿同列同义。
        let dividend_from = "FROM transactions t \
                         JOIN security_transactions st ON st.transaction_id = t.id AND st.action='dividend' \
                         JOIN accounts a ON a.id = t.account_id AND a.is_deleted = 0";

        let mut conditions: Vec<String> = vec!["t.is_deleted=0".to_string()];
        // 分红腿的过滤与已实现腿同源（ADR-0129 决策 3）：只多一条 kind 守卫，其余条件逐字复用。
        let mut dividend_conditions: Vec<String> = vec![
            "t.is_deleted=0".to_string(),
            "t.kind='dividend'".to_string(),
        ];
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(acct_id) = &filter.account_id {
            params.push(Box::new(acct_id.clone()));
            let condition = format!("t.account_id=?{}", params.len());
            conditions.push(condition.clone());
            dividend_conditions.push(condition);
        }
        if let Some(inst_id) = &filter.instrument_id {
            params.push(Box::new(inst_id.clone()));
            let condition = format!("st.instrument_id=?{}", params.len());
            conditions.push(condition.clone());
            dividend_conditions.push(condition);
        }

        // 同一份位置参数（`?N`）在单条语句内可重复引用，故两腿共用一份取数参数。
        let where_clause = format!(" WHERE {}", conditions.join(" AND "));
        let dividend_where_clause = format!(" WHERE {}", dividend_conditions.join(" AND "));

        // 汇总按匹配行币种分组（ADR-0107 决策 6）：不做跨币种折算，各币种小计独立成立——
        // 原「各币种裸数字直接 SUM」的混算口径废止（多币种账户下合计是错的）。
        // 逐匹配「卖出明细」查询已随明细卡退役（决策 1），本函数只产出三张汇总视图 + 分组总数。
        let total_sql = format!(
            "SELECT sls.currency_code, COALESCE(SUM(sls.realized_pnl_cents), 0) \
         {base_from}{where_clause} GROUP BY sls.currency_code ORDER BY sls.currency_code"
        );
        // 按年 / 按账户两表 = 两腿各自聚合后 UNION ALL，再按 (行键, 币种) 二次聚合
        // （ADR-0129 决策 1/3）：两腿先各自 GROUP BY 再合并，故「只有分红、没有卖出」的
        // 年份 / 账户同样成行——原口径下这类行整行不存在；合计在同一段 SQL 内相加。
        let year_sql = format!(
            "SELECT year, currency_code, \
                SUM(realized_pnl_cents), SUM(dividend_cents), \
                SUM(realized_pnl_cents + dividend_cents) \
         FROM ( \
             SELECT substr(t.date, 1, 4) AS year, sls.currency_code AS currency_code, \
                    SUM(sls.realized_pnl_cents) AS realized_pnl_cents, 0 AS dividend_cents \
             {base_from}{where_clause} GROUP BY year, sls.currency_code \
             UNION ALL \
             SELECT substr(t.date, 1, 4) AS year, t.currency_code AS currency_code, \
                    0 AS realized_pnl_cents, SUM(t.amount_cents) AS dividend_cents \
             {dividend_from}{dividend_where_clause} GROUP BY year, t.currency_code \
         ) GROUP BY year, currency_code ORDER BY year, currency_code"
        );
        let account_sql = format!(
            "SELECT account_id, account_name, currency_code, \
                SUM(realized_pnl_cents), SUM(dividend_cents), \
                SUM(realized_pnl_cents + dividend_cents) \
         FROM ( \
             SELECT a.id AS account_id, a.name AS account_name, sls.currency_code AS currency_code, \
                    SUM(sls.realized_pnl_cents) AS realized_pnl_cents, 0 AS dividend_cents \
             {base_from}{where_clause} GROUP BY a.id, a.name, sls.currency_code \
             UNION ALL \
             SELECT a.id AS account_id, a.name AS account_name, t.currency_code AS currency_code, \
                    0 AS realized_pnl_cents, SUM(t.amount_cents) AS dividend_cents \
             {dividend_from}{dividend_where_clause} GROUP BY a.id, a.name, t.currency_code \
         ) GROUP BY account_id, account_name, currency_code ORDER BY account_name, currency_code"
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
    })
}
