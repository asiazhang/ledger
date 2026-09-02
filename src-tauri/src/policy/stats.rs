//! 保单视角统计：实时推导保费、现金流入、下期扣款日与到期态。

use std::collections::HashMap;

use chrono::NaiveDate;
use rusqlite::Connection;

use crate::db::query::{FromRow, query_all};
use crate::error::Result;
use crate::models::PolicyStats;
use crate::transaction::amount::{
    Measure, contributing_kinds_sql, policy_inflow_expr, policy_premium_expr,
};

use super::validation::parse_date;

// ---------------------------------------------------------------------------
// 保单视角统计（issue #363 / ADR-0051 决策 5/6：实时推导，不落库）
// ---------------------------------------------------------------------------

/// 保单基础行（id + 保障期间止日）：到期推导只读这两列，列表序与
/// [`list_policies`] 一致（created_at, id）。
struct PolicyPeriodRow {
    id: String,
    end_date: Option<String>,
}

impl FromRow for PolicyPeriodRow {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PolicyPeriodRow {
            id: row.get(0)?,
            end_date: row.get(1)?,
        })
    }
}

/// 挂单流水逐保单合计行（kind 维度经 Amount 接缝矩阵驱动后的聚合结果）。
struct PolicySumRow {
    policy_id: String,
    native_cents: i64,
}

impl FromRow for PolicySumRow {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PolicySumRow {
            policy_id: row.get(0)?,
            native_cents: row.get(1)?,
        })
    }
}

/// 活跃缴费协议逐保单最早 pending 期次行（下期扣款日）。
struct PolicyNextChargeRow {
    policy_id: String,
    next_date: String,
}

impl FromRow for PolicyNextChargeRow {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PolicyNextChargeRow {
            policy_id: row.get(0)?,
            next_date: row.get(1)?,
        })
    }
}

/// 挂单流水逐保单合计：对给定度量表达式（与贡献 kind 过滤同出矩阵）按
/// `policy_id` 求本位币合计，只认未删除流水且保单未删除。
fn sum_by_policy(
    conn: &Connection,
    measure_expr: &str,
    kinds_sql: &str,
) -> Result<HashMap<String, i64>> {
    Ok(query_all::<PolicySumRow, _>(
        conn,
        &format!(
            "SELECT t.policy_id, SUM({measure_expr}) AS native_cents \
                 FROM transactions t JOIN policies p ON p.id = t.policy_id \
                 WHERE t.is_deleted=0 AND t.kind IN ({kinds_sql}) AND p.is_deleted=0 \
                 GROUP BY t.policy_id",
        ),
        [],
    )?
    .into_iter()
    .map(|r| (r.policy_id, r.native_cents))
    .collect())
}

/// conn 级聚合：逐保单视角统计（只读，实时推导不落库，issue #363）。
/// `today` 由命令层注入（本地今日），BDD 可传固定日期获得确定性到期口径。
///
/// - 累计已缴保费 / 累计现金流入：挂单流水（`policy_id` 归属，issue #361）忠实
///   合计 `amount_native_cents`——落库时已经 Writer 接缝折算本位币，读取期不二次
///   折算；kind→符号经 Amount 接缝矩阵驱动（不另写口径）；不摊销；软删流水
///   不计入（逐笔可对账）。
/// - 下期扣款日：该保单**活跃**缴费协议（订阅形态 active 段）的最早 pending 期次
///   （暂停/取消段不产生扣款预期，AC「无协议不显示」的同款语义）。
/// - 到期态：止日非空且早于 today → 已到期；止日空 = 长期/终身 → 恒 `false`
///   （可推导的状态不持久化，ADR-0051 决策 5）。
/// - 软删保单不产生统计行；其历史流水引用原样保留，且按 `policy_id` 分组天然
///   不串入其他保单统计。
pub fn policy_stats(conn: &Connection, today: NaiveDate) -> Result<Vec<PolicyStats>> {
    // 基础行：未删除保单（软删不进列表 → 也不进统计）。
    let periods: Vec<PolicyPeriodRow> = query_all(
        conn,
        "SELECT id, end_date FROM policies WHERE is_deleted=0 ORDER BY created_at, id",
        [],
    )?;

    // 挂单保费/流入合计：度量经 Amount 接缝 kind→度量矩阵驱动（与行级口径同源），
    // 两侧仅度量不同，聚合收口在同一辅助。
    let paid = sum_by_policy(
        conn,
        &policy_premium_expr("t"),
        &contributing_kinds_sql(Measure::PolicyPremium),
    )?;
    let inflow = sum_by_policy(
        conn,
        &policy_inflow_expr("t"),
        &contributing_kinds_sql(Measure::PolicyInflow),
    )?;

    // 下期扣款日：活跃订阅形态协议的最早 pending 期次（日期列 YYYY-MM-DD，
    // 字典序即时间序）；已取消协议的 pending 期次在取消时已批量转 cancelled，
    // 暂停段被 status='active' 排除——缓缴不产生扣款预期。
    let next_charges: HashMap<String, String> = query_all::<PolicyNextChargeRow, _>(
        conn,
        "SELECT sp.policy_id, MIN(o.scheduled_date) AS next_date \
         FROM scheduled_transaction_occurrences o \
         JOIN scheduled_transactions st ON st.id = o.scheduled_transaction_id \
         JOIN subscription_plans sp ON sp.scheduled_transaction_id = st.id \
         WHERE sp.policy_id IS NOT NULL AND st.is_deleted=0 \
           AND st.kind='subscription' AND st.status='active' \
           AND o.is_deleted=0 AND o.status='pending' \
         GROUP BY sp.policy_id",
        [],
    )?
    .into_iter()
    .map(|r| (r.policy_id, r.next_date))
    .collect();

    periods
        .into_iter()
        .map(|p| {
            // 到期推导：止日非空且早于 today → 已到期；止日空 = 长期/终身。
            // 止日格式由写路径校验（YYYY-MM-DD），脏数据在此报错上抛不静默跳过。
            let is_expired = match &p.end_date {
                Some(end) => {
                    let end_date = parse_date(end)?;
                    end_date < today
                }
                None => false,
            };
            let policy_id = p.id;
            let total_paid_native_cents = paid.get(&policy_id).copied().unwrap_or(0);
            let total_inflow_native_cents = inflow.get(&policy_id).copied().unwrap_or(0);
            let next_charge_date = next_charges.get(&policy_id).cloned();
            Ok(PolicyStats {
                policy_id,
                native_currency: crate::transaction::amount::default_currency_code().to_string(),
                total_paid_native_cents,
                total_inflow_native_cents,
                next_charge_date,
                is_expired,
            })
        })
        .collect()
}
