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
//! 推算成本口径（折算月/年成本）见 [`monthly_coefficient`]（issue #161）。

use std::collections::HashMap;

use chrono::{Datelike, Months, NaiveDate};
use rusqlite::Connection;
use serde::Serialize;

use crate::db::query::{FromRow, query_all};
use crate::error::Result;
use crate::scheduled_transactions::models::RecurrenceType;
use crate::transaction::amount;

/// 逐订阅行：计划基础信息 + 该订阅本月/本年实际花费（本位币）。
#[derive(Debug, Clone, Serialize)]
pub struct SubscriptionSpendRow {
    pub plan_id: String,
    pub note: Option<String>,
    /// 商户名（左联 merchants 现名，issue #190 / ADR-0028）：改名即时生效；
    /// 商户软删后历史计划照常显示原名（merchants 行仍保留）。
    pub merchant_name: Option<String>,
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

/// `subscription_spend_overview` 命令返回的订阅花费总览（本位币口径，单位：分）。
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
    /// 折算月成本合计（本位币，分）：只统计 active 计划，系数见 [`monthly_coefficient`]
    pub projected_month_native_cents: i64,
    /// 折算年成本合计（本位币，分）= 折算月成本 × 12；纯展示，不落库、不进流水与预算
    pub projected_year_native_cents: i64,
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

/// 计划基础信息（扩展表 merchant 左联，缺行为空）。
struct PlanBase {
    id: String,
    note: Option<String>,
    merchant_name: Option<String>,
    status: String,
    amount_cents: i64,
    currency_code: String,
}

impl FromRow for PlanBase {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PlanBase {
            id: row.get(0)?,
            note: row.get(1)?,
            merchant_name: row.get(2)?,
            status: row.get(3)?,
            amount_cents: row.get(4)?,
            currency_code: row.get(5)?,
        })
    }
}

/// 折算月成本系数（issue #161，ADR-0023 决策二）：后端单点收口。
///
/// 月付 ×1、年付 ÷12、周付 ×52÷12、日付 ×30；`recurrence_interval > 1` 时
/// 按间隔均摊（每 N 期一扣 → 系数 ÷ N，如「每 3 月 ¥300」折算月成本 ¥100）。
/// 表约束已保证 `recurrence_interval > 0`（建表 CHECK），直接除法即可。
fn monthly_coefficient(recurrence_type: RecurrenceType, recurrence_interval: i64) -> f64 {
    let per_cycle = match recurrence_type {
        RecurrenceType::Monthly => 1.0,
        RecurrenceType::Yearly => 1.0 / 12.0,
        RecurrenceType::Weekly => 52.0 / 12.0,
        RecurrenceType::Daily => 30.0,
    };
    per_cycle / recurrence_interval as f64
}

/// 推算中间行：active 订阅计划的计费参数。
struct PlanBilling {
    amount_cents: i64,
    currency_code: String,
    recurrence_type: String,
    recurrence_interval: i64,
}

impl FromRow for PlanBilling {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PlanBilling {
            amount_cents: row.get(0)?,
            currency_code: row.get(1)?,
            recurrence_type: row.get(2)?,
            recurrence_interval: row.get(3)?,
        })
    }
}

/// 推算成本（issue #161，ADR-0023 决策二）：按当前 active 订阅计划参数推算的
/// 持续烧钱速度，只算 active、不看执行情况；金额在计划币种上折算本位币
/// （缺汇率报错上抛）。纯展示口径：不落库、不进流水与预算。
///
/// 舍入口径：逐计划先折算本位币再乘系数、四舍五入到分，最后求和；
/// 折算年成本 = 折算月成本合计 × 12（不在年这一级再舍入）。
fn query_projected_cost(conn: &Connection) -> Result<(i64, i64)> {
    let plans: Vec<PlanBilling> = query_all(
        conn,
        "SELECT amount_cents, currency_code, recurrence_type, recurrence_interval \
         FROM scheduled_transactions \
         WHERE kind = 'subscription' AND status = 'active' AND is_deleted = 0",
        [],
    )?;
    let mut projected_month = 0i64;
    for p in plans {
        // 未知周期类型为脏数据，报错上抛，不静默跳过
        let recurrence_type: RecurrenceType = p.recurrence_type.parse()?;
        let native_cents = amount::convert_to_native(conn, p.amount_cents, &p.currency_code)?;
        let coefficient = monthly_coefficient(recurrence_type, p.recurrence_interval);
        projected_month += (native_cents as f64 * coefficient).round() as i64;
    }
    Ok((projected_month, projected_month * 12))
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
    // 商户名左联 merchants 现名：改名即时生效（引用指向 id）；商户软删后历史计划照常显示。
    let bases: Vec<PlanBase> = query_all(
        conn,
        "SELECT st.id, st.note, m.name, st.status, st.amount_cents, st.currency_code \
         FROM scheduled_transactions st \
         LEFT JOIN subscription_plans sp ON sp.scheduled_transaction_id = st.id \
         LEFT JOIN merchants m ON m.id = sp.merchant_id \
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
                merchant_name: b.merchant_name,
                status: b.status,
                amount_cents: b.amount_cents,
                currency_code: b.currency_code,
                this_month_native_cents: window_sum(&|m| m.month == this_month_key),
                this_year_native_cents: window_sum(&|m| m.month.starts_with(&this_year_prefix)),
            }
        })
        .collect();

    let (projected_month_native_cents, projected_year_native_cents) = query_projected_cost(conn)?;

    Ok(SubscriptionSpendOverview {
        native_currency: amount::default_currency_code().to_string(),
        this_month_native_cents,
        this_year_native_cents,
        months,
        rows,
        projected_month_native_cents,
        projected_year_native_cents,
    })
}
