//! 时点持仓（AsOfHolding，spec #168 / issue #218）：
//! 投资域核心推算不变量「仅认 buy/sell 流水、sell 取负、按交易日（含当日）前缀求和」
//! 的单点收敛模块。接口入口 [`holdings_as_of`]：给定连接、可选标的、交易日，
//! 返回该时点的持有数量（`instrument_id=None` 为全组合形态，所有标的数量之和）；
//! 账户维度扩展形态 [`holdings_as_of_in`]（issue #1195：资金加权收益率的边界
//! 市值需要（账户 × 标的）粒度的时点存量），单标的形态为其 `account_id=None`
//! 薄委托——推算口径仍单点住本模块。
//!
//! 契约要点（与 CONTEXT-investment「时点持仓（AsOfHolding）」词条一致）：
//!
//! - **时间语义是交易日**：`as_of_date` 为 ISO 交易日（YYYY-MM-DD，非法格式显式报错），
//!   推算取「交易日 ≤ as_of」的前缀求和（含当日）；周采样键（week_start）是
//!   PortfolioValueTrend 查询侧的时间语义，不进本模块——双时间键契约由此显式分界。
//! - **认 buy/sell、基金转换（convert）两腿与份额调整（split）腿**（CONTEXT 核心域
//!   Transaction Kind Mapping）：buy 取正、sell 取负后按日期前缀求和；convert 一笔两腿
//!   ——转出腿取负、转入腿取正（ADR-0099：同一交易行内两个方向的持仓变化，全组合
//!   形态两腿同时入账）；split 取 Δ（ADR-0106 决策 8：单标的**带符号**份额变动，
//!   `+Δ` 折算/结转/送股、`−Δ` 缩股同腿自然生效，见 #1049 / #1050）。dividend
//!   零份额变动，不进本推算（ADR-0109）。
//! - **排除软删除账户**（issue #217 定案）：流水口径过滤 `accounts.is_deleted = 0`，
//!   与 Holding（`v_holdings`）/ InvestedInstrument / 净资产全局对齐——删除账户 =
//!   从全部投资视角（含历史走势曲线）消失，账户删除/恢复（软删标志翻转）使流水
//!   自动进出推算，无需时点存续状态。隐藏账户不是软删除，与 `v_holdings` 一致不排除。
//! - **消费者**：PortfolioValueTrend 的组合市值（组合走势按标的分组增量推进，
//!   消费本模块的腿流投影 [`holdings_legs_by_instrument`]，issue #1654）；
//!   资金加权收益率的边界市值消费账户维度扩展形态（issue #1195）。单标的走势
//!   是价格直出，不消费本模块。

use chrono::NaiveDate;
use rusqlite::{Connection, params};
use std::collections::HashMap;

use ledger_infra::error::{AppError, Result};

/// 某标的（或全组合，`instrument_id=None`）在某交易日的持有数量。
///
/// 每次调用自取流水（单一函数、无预载会话变体，spec #168 定案第 3 条）：
/// 单条索引查询亚毫秒级，真慢了再加变体是纯增量。
pub fn holdings_as_of(
    conn: &Connection,
    instrument_id: Option<&str>,
    as_of_date: &str,
) -> Result<f64> {
    holdings_as_of_in(conn, None, instrument_id, as_of_date)
}

/// [`holdings_as_of`] 的账户维度扩展形态（issue #1195）：`account_id=None`
/// 即既有全组合 / 单标的形态（薄委托），`Some` 时只求和该账户名下的持仓变动
/// ——资金加权收益率的边界市值需要（账户 × 标的）粒度的时点存量。
pub fn holdings_as_of_in(
    conn: &Connection,
    account_id: Option<&str>,
    instrument_id: Option<&str>,
    as_of_date: &str,
) -> Result<f64> {
    NaiveDate::parse_from_str(as_of_date, "%Y-%m-%d").map_err(|_| {
        AppError::codedp(
            "instrument.as-of-date-invalid",
            format!("as-of 交易日格式无效: {as_of_date}"),
            &[as_of_date],
        )
    })?;

    // 口径内化为一条 SQL 的四臂 UNION ALL：仅认持仓变动流水（buy/sell）、
    // convert 两腿（行内两个方向）与 split 腿（带符号 Δ，ADR-0106 决策 8）、前缀求和
    // （含当日）；软删除账户与软删交易行一并排除（issue #217 定案、与 Holding
    // 同口径）；账户维度过滤随 ?3 生效（None = 不限账户）。
    //
    // convert 一笔一行两腿：转出腿（instrument_id）记 −quantity，转入腿
    // （to_instrument_id）记 +to_quantity；单标的查询时只取命中那腿（转出腿用
    // instrument_id 命中、转入腿用 to_instrument_id 命中），全组合（?2 IS NULL）
    // 时两腿同时入账——转换前后组合总份额不变。
    let sql = "SELECT COALESCE(SUM(qty), 0.0) FROM (\
                   SELECT CASE st.action WHEN 'buy' THEN st.quantity ELSE -st.quantity END AS qty \
                   FROM security_transactions st \
                   JOIN transactions t ON t.id = st.transaction_id \
                   JOIN accounts a ON a.id = t.account_id \
                   WHERE st.action IN ('buy','sell') \
                     AND st.quantity IS NOT NULL \
                     AND t.is_deleted = 0 \
                     AND a.is_deleted = 0 \
                     AND st.instrument_id = COALESCE(?2, st.instrument_id) \
                     AND (?3 IS NULL OR t.account_id = ?3) \
                     AND t.date <= ?1 \
                   UNION ALL \
                   SELECT -st.quantity AS qty \
                   FROM security_transactions st \
                   JOIN transactions t ON t.id = st.transaction_id \
                   JOIN accounts a ON a.id = t.account_id \
                   WHERE st.action = 'convert' \
                     AND st.quantity IS NOT NULL \
                     AND t.is_deleted = 0 \
                     AND a.is_deleted = 0 \
                     AND st.instrument_id = COALESCE(?2, st.instrument_id) \
                     AND (?3 IS NULL OR t.account_id = ?3) \
                     AND t.date <= ?1 \
                   UNION ALL \
                   SELECT st.to_quantity AS qty \
                   FROM security_transactions st \
                   JOIN transactions t ON t.id = st.transaction_id \
                   JOIN accounts a ON a.id = t.account_id \
                   WHERE st.action = 'convert' \
                     AND st.to_quantity IS NOT NULL \
                     AND t.is_deleted = 0 \
                     AND a.is_deleted = 0 \
                     AND st.to_instrument_id = COALESCE(?2, st.to_instrument_id) \
                     AND (?3 IS NULL OR t.account_id = ?3) \
                     AND t.date <= ?1 \
                   UNION ALL \
                   SELECT st.quantity AS qty \
                   FROM security_transactions st \
                   JOIN transactions t ON t.id = st.transaction_id \
                   JOIN accounts a ON a.id = t.account_id \
                   WHERE st.action = 'split' \
                     AND st.quantity IS NOT NULL \
                     AND t.is_deleted = 0 \
                     AND a.is_deleted = 0 \
                     AND st.instrument_id = COALESCE(?2, st.instrument_id) \
                     AND (?3 IS NULL OR t.account_id = ?3) \
                     AND t.date <= ?1) \
               ";
    let quantity: f64 =
        conn.query_row(sql, params![as_of_date, instrument_id, account_id], |r| {
            r.get(0)
        })?;
    Ok(quantity)
}

/// 一条持仓变动腿：`(交易日, 带符号数量增量)`——买入累加、卖出递减、convert
/// 转出腿取负/转入腿取正、split 带符号 Δ。组内按交易日升序扫描时逐腿累加即得
/// 任意时点的时点持仓——[`holdings_as_of`] 同一推算不变量的流水投影形态。
pub(crate) type HoldingsLegStream = HashMap<String, Vec<(String, f64)>>;

/// 全库持仓变动腿流，按标的分组、组内按交易日升序（SQL 层 `ORDER BY` 保序）。
///
/// 与 [`holdings_as_of`] 的四臂 UNION ALL 同形（同一推算不变量的两种投影）：
/// 仅认 buy/sell、convert 两腿与 split 腿，逐腿同样的 `quantity`/`to_quantity`
/// 非空条件、同样的软删除账户与软删交易行排除——差异只有两点：标的维度
///（分腿归属替代 `COALESCE(?2,…)` 钉定）与无日期上界（上界由消费方的游标
/// 推进承担，而非 SQL 谓词）。**两处 SQL 必须同步修改**：任何一侧口径变化
///（新增腿类型、过滤条件变化）必须同时改另一侧，并保持 `tests/holdings_as_of`
/// 的配对测试绿（同一夹具上逐标的逐日期：腿流前缀和 ≡ `holdings_as_of`）。
///
/// 消费方（组合走势，issue #1654）：价格行按标的分组、组内按交易日升序，
/// 游标扫过「交易日 ≤ 采样日」的前缀逐腿累加——替代逐价格行全量重算
///（周采样 × 标的的嵌套循环，50 万笔库 p95 8.85s）；相邻周点间不再重复
/// 扫描同一标的的全部交易。累计序为日期升序，与 SQL 逐行 `SUM` 的扫描序在
/// f64 末位可能相差 ULP 级；周点金额经分位取整后与逐行 as-of 口径逐点一致
///（配对测试与走势等价测试双钉）。
pub(crate) fn holdings_legs_by_instrument(conn: &Connection) -> Result<HoldingsLegStream> {
    // 四臂 UNION ALL 与 [`holdings_as_of`] 逐臂同形（见函数文档的同步纪律）：
    // 每行至多产出「转出腿 + 转入腿」两条腿（convert 一笔两腿），`ORDER BY`
    // 给出（标的，交易日）字典序，消费方按序分组即得组内升序腿流。
    let sql = "SELECT st.instrument_id, t.date, \
                   CASE st.action WHEN 'buy' THEN st.quantity ELSE -st.quantity END \
                   FROM security_transactions st \
                   JOIN transactions t ON t.id = st.transaction_id \
                   JOIN accounts a ON a.id = t.account_id \
                   WHERE st.action IN ('buy','sell') \
                     AND st.quantity IS NOT NULL \
                     AND t.is_deleted = 0 \
                     AND a.is_deleted = 0 \
               UNION ALL \
               SELECT st.instrument_id, t.date, -st.quantity \
                   FROM security_transactions st \
                   JOIN transactions t ON t.id = st.transaction_id \
                   JOIN accounts a ON a.id = t.account_id \
                   WHERE st.action = 'convert' \
                     AND st.quantity IS NOT NULL \
                     AND t.is_deleted = 0 \
                     AND a.is_deleted = 0 \
               UNION ALL \
               SELECT st.to_instrument_id, t.date, st.to_quantity \
                   FROM security_transactions st \
                   JOIN transactions t ON t.id = st.transaction_id \
                   JOIN accounts a ON a.id = t.account_id \
                   WHERE st.action = 'convert' \
                     AND st.to_quantity IS NOT NULL \
                     AND t.is_deleted = 0 \
                     AND a.is_deleted = 0 \
               UNION ALL \
               SELECT st.instrument_id, t.date, st.quantity \
                   FROM security_transactions st \
                   JOIN transactions t ON t.id = st.transaction_id \
                   JOIN accounts a ON a.id = t.account_id \
                   WHERE st.action = 'split' \
                     AND st.quantity IS NOT NULL \
                     AND t.is_deleted = 0 \
                     AND a.is_deleted = 0 \
               ORDER BY 1, 2";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, f64>(2)?,
        ))
    })?;
    // SQL 已按（标的，交易日）升序输出；push 保序即得组内升序。
    let mut by_instrument: HoldingsLegStream = HashMap::new();
    for row in rows {
        let (instrument_id, date, qty) = row?;
        by_instrument
            .entry(instrument_id)
            .or_default()
            .push((date, qty));
    }
    Ok(by_instrument)
}
