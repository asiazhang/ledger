//! 投资盈亏读投影（issue #1077 / #1078；ADR-0107 / ADR-0129 / ADR-0114 / ADR-0132）：
//! 已实现盈亏汇总与按币种累计收益查询的只读单点。
//!
//! - [`query_cumulative_pnl_summary`]：未实现盈亏（`v_holdings`）+ 已实现盈亏（卖出
//!   匹配）+ 累计分红三腿按币种独立成组，不跨币种折算；缺价 / 缺汇率持仓按空值跳过。
//! - [`query_realized_pnl_summary`]：盈亏页按年 / 按账户两表（已实现 + 分红两腿 +
//!   按年表的完整年度收益两腿）与按币种总数、按标的行；软删账户与软删流水排除、
//!   隐藏账户照常计入。
//! - [`query_holdings_summary_by_currency`]：持仓市值 / 未实现盈亏按账户币种分组合计。

use std::collections::{BTreeMap, HashMap};

use chrono::NaiveDate;
use rusqlite::Connection;

use super::as_of::AsOfValues;
use super::holdings::holdings_legs_by_account_and_instrument;
use super::lots::QTY_GUARD_EPSILON;
use super::model::{
    AccountPnl, CurrencyCumulativePnl, CurrencyHoldingTotals, CurrencyPnl, InstrumentPnl,
    PnlFilter, RealizedPnlSummary, YearPnl,
};
use ledger_infra::db::query::query_all;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;

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
    // 三腿 `UNION ALL` 后按币种分组求和：未实现腿按账户币种、已实现腿按匹配行币种、
    // 分红腿按交易行币种，三条口径的币种在单账户内同源（buy/sell 与 dividend 记录
    // 币种恒为账户币），故同组可直接相加。
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
                   UNION ALL \
                   SELECT t.currency_code AS currency_code, t.amount_cents AS amount_cents \
                   FROM transactions t \
                   JOIN security_transactions st ON st.transaction_id = t.id AND st.action='dividend' \
                   JOIN accounts a ON a.id = t.account_id AND a.is_deleted = 0 \
                   WHERE t.is_deleted = 0 AND t.kind='dividend' \
               ) GROUP BY currency_code ORDER BY currency_code";
    query_all(conn, sql, [])
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
/// 四查与年度收益腿装载（`attach_annual_return_legs`）整体收进同一读事务
/// （嵌套感知）——写提交落在语句之间会总量≠分量和，同屏口径自相矛盾。
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
        let mut by_year: Vec<YearPnl> = query_all(conn, &year_sql, params_ref.as_slice())?;
        let by_account: Vec<AccountPnl> = query_all(conn, &account_sql, params_ref.as_slice())?;
        let by_instrument: Vec<InstrumentPnl> =
            query_all(conn, &instrument_sql, params_ref.as_slice())?;

        // 完整年度收益两腿（ADR-0132 / issue #1535）：未实现变动与年度收益只增不改地
        // 追加到按年行，既有三列读数逐位不变。
        attach_annual_return_legs(conn, filter, &mut by_year)?;

        Ok(RealizedPnlSummary {
            total,
            by_year,
            by_account,
            by_instrument,
        })
    })
}

/// 按年行的未实现变动与年度收益（ADR-0132 / issue #1535，词汇表「年度收益
/// （AnnualReturn）」）：采用现金流式——
///
/// ```text
/// 年度收益 = (期末持仓市值 − 期初持仓市值) + 年内卖出净收入 + 年内分红 − 年内买入支出
/// 未实现变动 = 年度收益 − 已实现盈亏 − 分红
/// ```
///
/// 与三腿定义式（已实现 + 分红 + Δ未实现）在估值齐全时逐位相等（FIFO 已实现与
/// 批次成本共用同一锚点，代数上可互导），且不需要时点成本。口径要点：
///
/// - **边界时点**：期初 = 上一年 12-31、期末 = 当年 12-31（当年即今日，价格取
///   ≤ 该日最近周采样，未来日期自然夹到最新）；市值 = 时点持仓（[`holdings_legs_by_account_and_instrument`]
///   前缀和）× 周线价 × 同期汇率（[`AsOfValues`]，与资金加权收益率边界市值
///   同一装载器）。convert 无现金腿、split 不产生现金，公式天然覆盖。
/// - **年内买入支出 = 该年 buy 行金额合计**（含费用；含期初存量补记行——否则
///   补记年会把整笔存量市值虚计为年度收益）；卖出净收入 = sell 行金额合计（已扣费）。
/// - **按币种分组沿用 ADR-0107 决策 6**（按币种分组、不跨币种折算）：本腿组币种
///   = 账户币种（边界市值折到账户币，与匹配行 / 分红行币种同源），
///   不跨币种折算；账户 / 标的筛选对市值腿与既有两腿同源同过滤。
/// - **空值语义**：边界仍有持仓而市值缺料（缺价或缺汇率）→ 该年 `None`
///   （不可算，前端显式标注）；期初或期末持仓为零则无需估值，年内全平仓等
///   形态无价也可算。已实现与分红两腿不受行情影响，不可算年份照常出数。
/// - **行集不变**：只加列不加行（#1533 的行集纪律——只有卖出或分红的年份成行）。
fn attach_annual_return_legs(
    conn: &Connection,
    filter: &PnlFilter,
    by_year: &mut [YearPnl],
) -> Result<()> {
    if by_year.is_empty() {
        return Ok(());
    }

    // 1. 各年的期初 / 期末边界日（相邻年份共享同一 12-31：本年期末即次年期初）。
    let mut boundaries: BTreeMap<String, (NaiveDate, NaiveDate)> = BTreeMap::new();
    for row in by_year.iter() {
        let Ok(year) = row.year.parse::<i32>() else {
            continue;
        };
        let Some(start) = NaiveDate::from_ymd_opt(year - 1, 12, 31) else {
            continue;
        };
        let Some(end) = NaiveDate::from_ymd_opt(year, 12, 31) else {
            continue;
        };
        boundaries.insert(row.year.clone(), (start, end));
    }
    if boundaries.is_empty() {
        return Ok(());
    }

    // 2. 边界市值装载器：每个去重后的边界日装载一次（相邻年共享，免双重扫描）。
    let mut boundary_dates: Vec<NaiveDate> = boundaries
        .values()
        .flat_map(|(start, end)| [*start, *end])
        .collect();
    boundary_dates.sort();
    boundary_dates.dedup();
    let mut values: HashMap<NaiveDate, AsOfValues> = HashMap::new();
    for date in boundary_dates {
        values.insert(date, AsOfValues::load(conn, date)?);
    }

    // 3. 腿流一次装载：各边界日的（账户 × 标的）时点持仓由前缀求和给出。
    let legs = holdings_legs_by_account_and_instrument(conn)?;

    // 4. 投资账户币种表：软删排除、隐藏计入（与已实现 / 分红腿同口径）。
    //    组币种 = 账户币种（与匹配行 / 分红行币种同源，分组不折算口径沿用
    //    ADR-0107 决策 6），故每个币种组只估值该币种账户。
    let mut account_conditions: Vec<String> = vec![
        "is_deleted = 0".to_string(),
        "type = 'investment'".to_string(),
    ];
    let mut account_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(acct_id) = &filter.account_id {
        account_params.push(Box::new(acct_id.clone()));
        account_conditions.push(format!("id = ?{}", account_params.len()));
    }
    let accounts_sql = format!(
        "SELECT id, currency_code FROM accounts WHERE {}",
        account_conditions.join(" AND ")
    );
    let account_refs: Vec<&dyn rusqlite::ToSql> =
        account_params.iter().map(|b| b.as_ref()).collect();
    let mut accounts_by_currency: HashMap<String, Vec<String>> = HashMap::new();
    {
        let mut stmt = conn.prepare(&accounts_sql)?;
        let rows = stmt.query_map(account_refs.as_slice(), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (id, currency) = row?;
            accounts_by_currency.entry(currency).or_default().push(id);
        }
    }

    // 5. 年内买卖现金流（按年 × 币种）：买入计支出（含期初存量补记行，它们是
    //    buy 行）、卖出为净收入；筛选与既有两腿同源同过滤（ADR-0129 决策 3）。
    let mut flow_conditions: Vec<String> = vec![
        "t.kind IN ('buy','sell')".to_string(),
        "t.is_deleted = 0".to_string(),
    ];
    let mut flow_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(acct_id) = &filter.account_id {
        flow_params.push(Box::new(acct_id.clone()));
        flow_conditions.push(format!("t.account_id = ?{}", flow_params.len()));
    }
    let instrument_join = if let Some(inst_id) = &filter.instrument_id {
        flow_params.push(Box::new(inst_id.clone()));
        flow_conditions.push(format!("st.instrument_id = ?{}", flow_params.len()));
        " JOIN security_transactions st ON st.transaction_id = t.id".to_string()
    } else {
        String::new()
    };
    let flows_sql = format!(
        "SELECT substr(t.date, 1, 4) AS year, t.currency_code, \
         SUM(CASE WHEN t.kind = 'buy' THEN t.amount_cents ELSE 0 END), \
         SUM(CASE WHEN t.kind = 'sell' THEN t.amount_cents ELSE 0 END) \
         FROM transactions t{instrument_join} \
         JOIN accounts a ON a.id = t.account_id AND a.is_deleted = 0 \
         WHERE {} GROUP BY year, t.currency_code",
        flow_conditions.join(" AND ")
    );
    let flow_refs: Vec<&dyn rusqlite::ToSql> = flow_params.iter().map(|b| b.as_ref()).collect();
    let mut flows: HashMap<(String, String), (i64, i64)> = HashMap::new();
    {
        let mut stmt = conn.prepare(&flows_sql)?;
        let rows = stmt.query_map(flow_refs.as_slice(), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                r.get::<_, Option<i64>>(3)?.unwrap_or(0),
            ))
        })?;
        for row in rows {
            let (year, currency, buy, sell) = row?;
            flows.insert((year, currency), (buy, sell));
        }
    }

    // 6. 逐行装配：先累计两个边界日的持仓市值（任一缺料 → 整行不可算，保持
    //    None 不按零计），再由现金流式导出两腿；已实现 / 分红两腿不动（读数
    //    逐位不变）。年内全平仓等边界持仓为零的形态无需估值，无价也可算。
    for row in by_year.iter_mut() {
        let Some((start, end)) = boundaries.get(&row.year) else {
            continue;
        };
        let (Some(start_values), Some(end_values)) = (values.get(start), values.get(end)) else {
            continue;
        };
        // 组币种无投资账户（如分红只到账银行卡）时按空持仓处理——可算、市值腿
        // 为 0，与「边界持仓为零则无需估值」同一语义；不特判会让这类年份被误判
        // 为不可算。
        let account_ids = accounts_by_currency
            .get(&row.currency_code)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut start_mv = 0i64;
        let mut end_mv = 0i64;
        let mut computable = true;
        for ((account_id, instrument_id), pair_legs) in legs.iter() {
            if !account_ids.contains(account_id) {
                continue;
            }
            if let Some(want) = &filter.instrument_id
                && instrument_id != want
            {
                continue;
            }
            let qty_start = quantity_at(pair_legs, &start.to_string());
            if qty_start.abs() > QTY_GUARD_EPSILON {
                match start_values.market_value(qty_start, instrument_id, &row.currency_code) {
                    Some(v) => start_mv += v,
                    None => computable = false,
                }
            }
            let qty_end = quantity_at(pair_legs, &end.to_string());
            if qty_end.abs() > QTY_GUARD_EPSILON {
                match end_values.market_value(qty_end, instrument_id, &row.currency_code) {
                    Some(v) => end_mv += v,
                    None => computable = false,
                }
            }
        }
        if !computable {
            continue;
        }
        let (buy, sell) = flows
            .get(&(row.year.clone(), row.currency_code.clone()))
            .copied()
            .unwrap_or((0, 0));
        let unrealized = end_mv - start_mv + sell - buy - row.realized_pnl_cents;
        row.unrealized_change_cents = Some(unrealized);
        row.annual_return_cents = Some(row.realized_gain_cents + unrealized);
    }
    Ok(())
}

/// 边界日的时点持仓：腿流按交易日升序，取「交易日 ≤ 边界日」的前缀和（含当日）
/// ——与 [`crate::holdings::holdings_as_of`] 同一前缀语义；单组腿数量级小
///（标的全部流水），线性过滤即可，无需游标增量。
fn quantity_at(legs: &[(String, f64)], as_of_date: &str) -> f64 {
    legs.iter()
        .filter(|(date, _)| date.as_str() <= as_of_date)
        .map(|(_, qty)| qty)
        .sum()
}
