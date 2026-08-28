//! 订阅花费——实际花费口径（issue #160，ADR-0023 决策二）。
//!
//! 实际花费 = 某日历月/年内，由订阅计划期次生成的交易流水的忠实合计：
//! - 金额读期次关联流水（`scheduled_transaction_occurrences.transaction_id`）的
//!   `amount_native_cents`——落库时已经 Writer 接缝折算为本位币，读取期不二次折算；
//! - 按流水 `date`（= 期次 `scheduled_date`）的日历月/年聚合，**不摊销**——
//!   年付订阅在扣款月全额计入，其余月份为 0；
//! - 不过滤计划状态：取消/暂停计划的历史实际花费如实保留；
//! - 只读聚合（先例：`dashboard_overview`），不新增任何写路径。
//!
//! 推算成本口径（折算月/年成本）不在本模块（issue #161）。

use std::collections::HashMap;

use chrono::{Datelike, Months, NaiveDate};
use rusqlite::Connection;
use serde::Serialize;

use crate::db::query::{FromRow, query_all};
use crate::error::Result;
use crate::transaction::amount;

/// 逐订阅行：计划基础信息 + 该订阅本月/本年实际花费（本位币）。
#[derive(Debug, Clone, Serialize)]
pub struct SubscriptionSpendRow {
    pub plan_id: String,
    pub note: Option<String>,
    pub counterparty: Option<String>,
    /// 计划状态（active/paused/cancelled/completed），历史花费不受其影响。
    pub status: String,
    /// 每期金额（计划币种，原始口径）。
    pub amount_cents: i64,
    pub currency_code: String,
    /// 该订阅本月实际花费（本位币，分）。
    pub this_month_native_cents: i64,
    /// 该订阅本年实际花费（本位币，分）。
    pub this_year_native_cents: i64,
}

/// 单个日历月的订阅实际花费（本位币，分）。
#[derive(Debug, Clone, Serialize)]
pub struct SubscriptionMonthSpend {
    /// 日历月，`YYYY-MM`。
    pub month: String,
    pub native_cents: i64,
}

/// `subscription_spend_overview` 命令返回的订阅实际花费总览（本位币口径，单位：分）。
#[derive(Debug, Clone, Serialize)]
pub struct SubscriptionSpendOverview {
    /// 折算基准币种（全局默认币种）
    pub native_currency: String,
    /// 本月实际花费合计（本位币，分）
    pub this_month_native_cents: i64,
    /// 本年实际花费合计（本位币，分）
    pub this_year_native_cents: i64,
    /// 过去 12 个日历月逐月实际花费（含当月，旧→新，无扣款月补 0）
    pub months: Vec<SubscriptionMonthSpend>,
    /// 逐订阅行（含已取消/暂停计划，其历史实际花费如实保留）
    pub rows: Vec<SubscriptionSpendRow>,
}

/// 聚合中间行：计划 × 日历月 → 本位币花费合计。
struct PlanMonthSpend {
    plan_id: String,
    month: String,
    native_cents: i64,
}

impl FromRow for PlanMonthSpend {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PlanMonthSpend {
            plan_id: row.get(0)?,
            month: row.get(1)?,
            native_cents: row.get(2)?,
        })
    }
}

/// 计划基础信息（扩展表 counterparty 左联，缺行为空）。
struct PlanBase {
    id: String,
    note: Option<String>,
    counterparty: Option<String>,
    status: String,
    amount_cents: i64,
    currency_code: String,
}

impl FromRow for PlanBase {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PlanBase {
            id: row.get(0)?,
            note: row.get(1)?,
            counterparty: row.get(2)?,
            status: row.get(3)?,
            amount_cents: row.get(4)?,
            currency_code: row.get(5)?,
        })
    }
}

/// conn 级聚合：订阅实际花费总览（只读）。`today` 由命令层注入（本地今日），
/// 单测与 e2e 可传固定日期获得确定性口径。
pub fn query_subscription_spend(
    conn: &Connection,
    today: NaiveDate,
) -> Result<SubscriptionSpendOverview> {
    // 期次关联流水的逐计划逐月本位币合计：只认 completed 期次真实生成的流水，
    // 计划状态不参与过滤（取消/暂停不影响历史实际花费）。
    let spends: Vec<PlanMonthSpend> = query_all(
        conn,
        "SELECT st.id, substr(t.date,1,7) AS month, SUM(t.amount_native_cents) AS native_cents \
         FROM scheduled_transaction_occurrences o \
         JOIN scheduled_transactions st ON st.id = o.scheduled_transaction_id \
         JOIN transactions t ON t.id = o.transaction_id \
         WHERE st.kind = 'subscription' AND o.status = 'completed' \
           AND o.is_deleted = 0 AND st.is_deleted = 0 AND t.is_deleted = 0 \
         GROUP BY st.id, month",
        [],
    )?;

    let mut by_plan: HashMap<String, HashMap<String, i64>> = HashMap::new();
    for s in spends {
        by_plan
            .entry(s.plan_id)
            .or_default()
            .insert(s.month, s.native_cents);
    }

    // 过去 12 个日历月（含当月，旧→新）：chrono Months 做月份裁剪（月末钳制），
    // 与 advance_date 的周期推进语义一致。
    let first = today - Months::new(11);
    let mut months = Vec::with_capacity(12);
    let mut cursor = first;
    for _ in 0..12 {
        months.push(SubscriptionMonthSpend {
            month: format!("{:04}-{:02}", cursor.year(), cursor.month()),
            native_cents: 0,
        });
        cursor = cursor + Months::new(1);
    }

    let this_month_key = format!("{:04}-{:02}", today.year(), today.month());
    let this_year_prefix = format!("{:04}", today.year());

    let mut this_month_native_cents = 0i64;
    let mut this_year_native_cents = 0i64;
    for m in &mut months {
        for plan_map in by_plan.values() {
            if let Some(cents) = plan_map.get(&m.month) {
                m.native_cents += cents;
            }
        }
        if m.month == this_month_key {
            this_month_native_cents = m.native_cents;
        }
        if m.month.starts_with(&this_year_prefix) {
            this_year_native_cents += m.native_cents;
        }
    }

    // 逐订阅行：全部订阅计划（不过滤状态），行内花费只汇总 12 个月窗口内的数据。
    let bases: Vec<PlanBase> = query_all(
        conn,
        "SELECT st.id, st.note, sp.counterparty, st.status, st.amount_cents, st.currency_code \
         FROM scheduled_transactions st \
         LEFT JOIN subscription_plans sp ON sp.scheduled_transaction_id = st.id \
         WHERE st.kind = 'subscription' AND st.is_deleted = 0 \
         ORDER BY st.created_at DESC",
        [],
    )?;

    let rows = bases
        .into_iter()
        .map(|b| {
            // 行内花费与顶层汇总同源：都从同一个 12 个月窗口推导（窗口必含本年至今），
            // 避免行级另走全史路径导致口径分叉。
            let plan_map = by_plan.get(&b.id);
            let month_cents =
                |month: &str| -> i64 { plan_map.and_then(|m| m.get(month)).copied().unwrap_or(0) };
            let window_sum = |pred: &dyn Fn(&SubscriptionMonthSpend) -> bool| -> i64 {
                months
                    .iter()
                    .filter(|m| pred(m))
                    .map(|m| month_cents(&m.month))
                    .sum()
            };
            SubscriptionSpendRow {
                plan_id: b.id,
                note: b.note,
                counterparty: b.counterparty,
                status: b.status,
                amount_cents: b.amount_cents,
                currency_code: b.currency_code,
                this_month_native_cents: window_sum(&|m| m.month == this_month_key),
                this_year_native_cents: window_sum(&|m| m.month.starts_with(&this_year_prefix)),
            }
        })
        .collect();

    Ok(SubscriptionSpendOverview {
        native_currency: amount::default_currency_code().to_string(),
        this_month_native_cents,
        this_year_native_cents,
        months,
        rows,
    })
}
