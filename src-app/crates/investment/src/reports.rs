//! 投资盈亏读投影（issue #1077 / #1078；ADR-0107 / ADR-0129 / ADR-0114）：
//! 已实现盈亏汇总与按币种累计收益查询的只读单点。
//!
//! - [`query_cumulative_pnl_summary`]：未实现盈亏（`v_holdings`）+ 已实现盈亏（卖出
//!   匹配）+ 累计分红三腿按币种独立成组，不跨币种折算；缺价 / 缺汇率持仓按空值跳过。
//! - [`query_cumulative_pnl_native_total`]：同三腿的折本位币单值（issue #1797，
//!   持仓页签合计卡与首页投资概览卡消费面）。
//! - [`query_realized_pnl_summary`]：盈亏页按年 / 按账户两张单值表（已实现 + 分红
//!   两腿逐腿按事件周汇率折本位币，#1845）；软删账户与软删流水排除、隐藏账户照常计入。
//! - [`query_holdings_summary_by_currency`]：持仓市值 / 未实现盈亏按账户币种分组合计。

use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use rusqlite::Connection;

use ledger_transaction::amount;

use super::fx_week::{FxWeekHistory, load_fx_week_history, nearest_week_fx_rate};
use super::model::{
    AccountPnl, CumulativePnlNativeTotal, CurrencyCumulativePnl, CurrencyHoldingTotals, PnlFilter,
    RealizedPnlSummary, YearPnl,
};
use ledger_infra::db::query::{FromRow, query_all};
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::{AppError, Result};

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

/// 单腿折算输入：(币种, 事件周) → 分合计（事件周为该组全部匹配所在周的周键）。
/// BTreeMap 保序使错误路径按 (币种, 周) 字典序首个失败点报错，不随哈希序漂移。
type LegSums = BTreeMap<(String, String), i64>;

/// 单腿分组集：行键（按年 = [year]；按账户 = [id, name]）→ 折算输入组。年行按
/// 年度升序出数。
type LegGroups = BTreeMap<Vec<String>, LegSums>;

/// 单腿取数：行键 + (币种, 事件周) 二次聚合（`sql` 的前 `key_cols` 列为行键，
/// 随后为币种、事件周键、分合计）。事件周键在 SQL 内派生（`date(t.date,'-6
/// days','weekday 1')`，与 `week_start` 生成列同式）；畸形日期派生 NULL 周键，
/// 以空串占位、折算侧按缺料处理，不静默给数。
fn fetch_leg_groups(
    conn: &Connection,
    sql: &str,
    key_cols: usize,
    params: &[&dyn rusqlite::ToSql],
) -> Result<LegGroups> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, |row| {
        let mut key = Vec::with_capacity(key_cols);
        for col in 0..key_cols {
            key.push(row.get::<_, String>(col)?);
        }
        let currency: String = row.get(key_cols)?;
        let week: Option<String> = row.get(key_cols + 1)?;
        let total: i64 = row.get::<_, Option<i64>>(key_cols + 2)?.unwrap_or(0);
        Ok((key, currency, week.unwrap_or_default(), total))
    })?;
    let mut groups = LegGroups::new();
    for row in rows {
        let (key, currency, week, total) = row?;
        *groups
            .entry(key)
            .or_default()
            .entry((currency, week))
            .or_insert(0) += total;
    }
    Ok(groups)
}

/// 单腿折本位币合计：逐 (币种, 事件周) 组按事件周（±8 周就近兜底）折算后求和，
/// 组内汇率同值、只舍入一次。兜底窗口内无点时分两路：币对（正反向）零历史 →
/// `fx.rate-missing` 码化错误（指路行情同步）；币对有历史但不在窗口 → `None`
/// （该腿缺料，调用方整行显式「无法计算」）。
fn fold_leg_native(fx: &FxWeekHistory, native: &str, groups: &LegSums) -> Result<Option<i64>> {
    let mut total = 0i64;
    for ((currency, week), cents) in groups {
        let rate = NaiveDate::parse_from_str(week, "%Y-%m-%d")
            .ok()
            .and_then(|date| nearest_week_fx_rate(fx, currency, native, date));
        match rate {
            Some(rate) => total += (*cents as f64 * rate).round() as i64,
            None => {
                if pair_has_no_history(fx, currency, native) {
                    return Err(missing_history_pair_error(currency, native));
                }
                return Ok(None);
            }
        }
    }
    Ok(Some(total))
}

/// 单行的两腿折算与主值合成：两腿各自可折算 → (主值, 两腿) 全 Some；任一腿
/// 缺料 → 三项全 None（行显式「无法计算」——不以零计入、不给半截数字）。
fn assemble_row(
    fx: &FxWeekHistory,
    native: &str,
    realized: &LegSums,
    dividend: &LegSums,
) -> Result<(Option<i64>, Option<i64>, Option<i64>)> {
    let realized = fold_leg_native(fx, native, realized)?;
    let dividend = fold_leg_native(fx, native, dividend)?;
    let gain = match (realized, dividend) {
        (Some(realized), Some(dividend)) => Some(realized + dividend),
        _ => None,
    };
    // 缺料行三项全 None：不在「无法计算」行上携带任何可读数字（含另一腿的
    // 真实值或 0）——半截数字由契约面排除，而非只靠渲染约束。
    if gain.is_none() {
        return Ok((None, None, None));
    }
    Ok((gain, realized, dividend))
}

/// 币对（正反向）在汇率历史中零行。
fn pair_has_no_history(fx: &FxWeekHistory, base: &str, quote: &str) -> bool {
    !fx.contains_key(&(base.to_string(), quote.to_string()))
        && !fx.contains_key(&(quote.to_string(), base.to_string()))
}

/// 币对零历史的 `fx.rate-missing`：码与插值参数沿用既有错误面（base/quote），
/// 中文原文与错误模板（errors.json `fx.rate-missing`）逐字一致——历史可由
/// 行情同步补齐，指路设置页「币种」页签（区别于整周空缺的「重试无济于事」分支）。
fn missing_history_pair_error(base: &str, quote: &str) -> AppError {
    AppError::codedp(
        "fx.rate-missing",
        format!(
            "未找到 {base} -> {quote} 的汇率（正反向均无），可在设置页「币种」页签同步汇率后重试"
        ),
        &[base, quote],
    )
}

/// 盈亏页读投影（ADR-0107 / ADR-0129 / #1845 单值翻案）：按年与按账户两张
/// 单值表——每行一个折全局默认币种（DefaultCurrency）的已实现收益单值
/// （行标识 + 主值 + 已实现盈亏 / 现金分红两腿拆解 + native_currency，词汇表
/// 「已实现收益（RealizedGain）」）。逐腿按自身事件周汇率折算
/// （[`nearest_week_fx_rate`]）：卖出匹配按卖出周、现金分红按到账周；事件周
/// 缺失在 ±8 周窗口内就近兜底（本投影新增的唯一折算语义，不回灌其他读路径）；
/// 窗口内仍无点 → 该腿缺料、整行显式「无法计算」；币对零历史沿用
/// `fx.rate-missing` 码化错误指路行情同步。分红腿与已实现腿同源同过滤
/// （ADR-0129 决策 3）：账户 / 标的筛选对两腿各用一次、软删账户与软删流水
/// 排除、隐藏账户照常计入；行键由（年度，币种）/（账户，币种）收窄为
/// 年度 / 账户（币种维退役），按年升序、按账户（名, id）序。
///
/// **读快照一致性（issue #1699）**：汇率装载与两表四查整体收进同一读事务
/// （嵌套感知）——写提交落在语句之间会两表各见一套数，同屏口径自相矛盾。
pub fn query_realized_pnl_summary(
    conn: &Connection,
    filter: &PnlFilter,
) -> Result<RealizedPnlSummary> {
    ensure_transaction(conn, || {
        let native = amount::default_currency_code(conn)?;
        let fx = load_fx_week_history(conn)?;

        // 账户软删在 JOIN 条件排除（issue #217 定案「删除账户 = 从全部投资视角消失」，
        // 与 v_holdings / 时点持仓读口径对齐）；交易行软删（t.is_deleted）同样排除——
        // sell 删除自 ADR-0097 起回补持仓并清空其匹配行，此处的读口径排除保留以覆盖
        // 旧版本遗留的幽灵匹配（issue #940）；隐藏账户不是软删除，照常计入。
        let realized_from = "FROM security_lot_sales sls \
                     JOIN transactions t ON t.id = sls.sell_transaction_id \
                     JOIN security_transactions st ON st.transaction_id = sls.sell_transaction_id \
                     JOIN accounts a ON a.id = t.account_id AND a.is_deleted = 0";
        // 分红腿的取数面（ADR-0109：kind 与扩展行双条件同 [`query_cumulative_pnl_summary`]）；
        // 两腿的标的过滤都直接落在扩展行 instrument_id 上，同列同义、无需 JOIN 字典。
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

        // 两表 = 两腿各自按（行键, 币种, 事件周）分组取数（ADR-0129 决策 1/3 的
        // 「各自聚合」纪律沿用）：只有分红、没有卖出的年份 / 账户同样成行。
        let week_of = "date(t.date, '-6 days', 'weekday 1')";
        let realized_year_sql = format!(
            "SELECT substr(t.date, 1, 4), sls.currency_code, {week_of}, SUM(sls.realized_pnl_cents) \
             {realized_from}{where_clause} \
             GROUP BY substr(t.date, 1, 4), sls.currency_code, {week_of}"
        );
        let dividend_year_sql = format!(
            "SELECT substr(t.date, 1, 4), t.currency_code, {week_of}, SUM(t.amount_cents) \
             {dividend_from}{dividend_where_clause} \
             GROUP BY substr(t.date, 1, 4), t.currency_code, {week_of}"
        );
        let realized_account_sql = format!(
            "SELECT a.id, a.name, sls.currency_code, {week_of}, SUM(sls.realized_pnl_cents) \
             {realized_from}{where_clause} \
             GROUP BY a.id, a.name, sls.currency_code, {week_of}"
        );
        let dividend_account_sql = format!(
            "SELECT a.id, a.name, t.currency_code, {week_of}, SUM(t.amount_cents) \
             {dividend_from}{dividend_where_clause} \
             GROUP BY a.id, a.name, t.currency_code, {week_of}"
        );

        let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();

        let realized_year = fetch_leg_groups(conn, &realized_year_sql, 1, &params_ref)?;
        let dividend_year = fetch_leg_groups(conn, &dividend_year_sql, 1, &params_ref)?;
        let realized_account = fetch_leg_groups(conn, &realized_account_sql, 2, &params_ref)?;
        let dividend_account = fetch_leg_groups(conn, &dividend_account_sql, 2, &params_ref)?;

        // 行键 = 两腿键集之并（只在一腿出现的行键同样成行）；按年 BTreeMap 序即
        // 年度升序，按账户出数后按（账户名, id）重排。空腿取空集 = 该腿为 0——
        // 无匹配是「零」，与「缺料」（None）两回事。
        let empty = BTreeMap::new();
        let mut year_keys: BTreeSet<&Vec<String>> = realized_year.keys().collect();
        year_keys.extend(dividend_year.keys());
        let mut by_year = Vec::with_capacity(year_keys.len());
        for key in &year_keys {
            let (gain, realized, dividend) = assemble_row(
                &fx,
                &native,
                realized_year.get(*key).unwrap_or(&empty),
                dividend_year.get(*key).unwrap_or(&empty),
            )?;
            by_year.push(YearPnl {
                year: key[0].clone(),
                native_currency: native.clone(),
                realized_pnl_cents: realized,
                dividend_cents: dividend,
                realized_gain_cents: gain,
            });
        }

        let mut account_keys: BTreeSet<&Vec<String>> = realized_account.keys().collect();
        account_keys.extend(dividend_account.keys());
        let mut by_account = Vec::with_capacity(account_keys.len());
        for key in &account_keys {
            let (gain, realized, dividend) = assemble_row(
                &fx,
                &native,
                realized_account.get(*key).unwrap_or(&empty),
                dividend_account.get(*key).unwrap_or(&empty),
            )?;
            by_account.push(AccountPnl {
                account_id: key[0].clone(),
                account_name: key[1].clone(),
                native_currency: native.clone(),
                realized_pnl_cents: realized,
                dividend_cents: dividend,
                realized_gain_cents: gain,
            });
        }
        by_account.sort_by(|x, y| {
            x.account_name
                .cmp(&y.account_name)
                .then_with(|| x.account_id.cmp(&y.account_id))
        });

        Ok(RealizedPnlSummary {
            by_year,
            by_account,
        })
    })
}
