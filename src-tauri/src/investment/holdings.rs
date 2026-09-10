//! 时点持仓（AsOfHolding，spec #168 / issue #218）：
//! 投资域核心推算不变量「仅认 buy/sell 流水、sell 取负、按交易日（含当日）前缀求和」
//! 的单点收敛模块。接口仅 [`holdings_as_of`] 一个入口：给定连接、可选标的、交易日，
//! 返回该时点的持有数量（`instrument_id=None` 为全组合形态，所有标的数量之和）。
//!
//! 契约要点（与 CONTEXT-investment「时点持仓（AsOfHolding）」词条一致）：
//!
//! - **时间语义是交易日**：`as_of_date` 为 ISO 交易日（YYYY-MM-DD，非法格式显式报错），
//!   推算取「交易日 ≤ as_of」的前缀求和（含当日）；周采样键（week_start）是
//!   PortfolioValueTrend 查询侧的时间语义，不进本模块——双时间键契约由此显式分界。
//! - **认 buy/sell 与基金转换（convert）流水**（CONTEXT 核心域 Transaction Kind Mapping）：
//!   buy 取正、sell 取负后按日期前缀求和；convert 一笔两腿——转出腿取负、转入腿取正
//!   （ADR-0099：同一交易行内两个方向的持仓变化，全组合形态两腿同时入账）。
//!   dividend/split 目前被写入层显式拒绝故无影响；未来公司行为落地时只改本模块内部，
//!   走势查询不感知。
//! - **排除软删除账户**（issue #217 定案）：流水口径过滤 `accounts.is_deleted = 0`，
//!   与 Holding（`v_holdings`）/ InvestedInstrument / 净资产全局对齐——删除账户 =
//!   从全部投资视角（含历史走势曲线）消失，账户删除/恢复（软删标志翻转）使流水
//!   自动进出推算，无需时点存续状态。隐藏账户不是软删除，与 `v_holdings` 一致不排除。
//! - **消费者**：PortfolioValueTrend 的组合市值（逐价格行取该标的当期数量，
//!   contract 阶段 issue #219 接线）；单标的走势是价格直出，不消费本模块。

use chrono::NaiveDate;
use rusqlite::{Connection, params};

use crate::error::{AppError, Result};

/// 某标的（或全组合，`instrument_id=None`）在某交易日的持有数量。
///
/// 每次调用自取流水（单一函数、无预载会话变体，spec #168 定案第 3 条）：
/// 单条索引查询亚毫秒级，真慢了再加变体是纯增量。
pub fn holdings_as_of(
    conn: &Connection,
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

    // 口径内化为一条 SQL 的三臂 UNION ALL：仅认持仓变动流水（buy/sell）与
    // convert 两腿（行内两个方向）、前缀求和（含当日）；软删除账户与软删交易行
    // 一并排除（issue #217 定案、与 Holding 同口径）。
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
                     AND t.date <= ?1) \
               ";
    let quantity: f64 = conn.query_row(sql, params![as_of_date, instrument_id], |r| r.get(0))?;
    Ok(quantity)
}
