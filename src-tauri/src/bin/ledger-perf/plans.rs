use chrono::{Datelike, Duration, Months, NaiveDate};
use rusqlite::Connection;

use super::generate::date_millis;
use super::generate::{AccountRow, DEVICE_ID, GenCounts};
use super::rng::{Rng, time_ordered_id};

const OCCURRENCE_HOUR: &str = "09:30:00";

const BUDGET_TOTAL: usize = 6;
const BUDGET_MONTHLY: usize = 4;
const BUDGET_MONTHLY_RANGE: (i64, i64) = (200_000, 900_000);
const BUDGET_YEARLY_RANGE: (i64, i64) = (2_000_000, 9_000_000);

const PENDING_HORIZON_MONTHS: u32 = 3;

pub(crate) fn insert_budgets(
    conn: &Connection,
    rng: &mut Rng,
    expense_pool: &[String],
    start_date: NaiveDate,
    counts: &mut GenCounts,
) -> Result<(), String> {
    let sql = "INSERT INTO budgets (id,category_id,period,amount_cents,start_date,\
         created_at,updated_at,version,device_id,is_deleted)\
         VALUES (?1,?2,?3,?4,?5,?6,?6,1,?7,0)";
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let stamp = format!("{start_date}T08:00:00Z");
    let millis = date_millis(&start_date);
    let n = BUDGET_TOTAL.min(expense_pool.len());
    for (idx, category_id) in expense_pool.iter().enumerate().take(n) {
        let (period, amount) = if idx < BUDGET_MONTHLY {
            (
                "monthly",
                rng.range_i64(BUDGET_MONTHLY_RANGE.0, BUDGET_MONTHLY_RANGE.1),
            )
        } else {
            (
                "yearly",
                rng.range_i64(BUDGET_YEARLY_RANGE.0, BUDGET_YEARLY_RANGE.1),
            )
        };
        stmt.execute(rusqlite::params![
            time_ordered_id("budgets", idx as u64, millis),
            category_id,
            period,
            amount,
            start_date.to_string(),
            stamp,
            DEVICE_ID,
        ])
        .map_err(|e| e.to_string())?;
    }
    counts.budgets = n;
    Ok(())
}

struct PlanSpec {
    kind: &'static str,
    label: &'static str,
    months_ago: u32,
    total_occurrences: Option<u32>,
    status: &'static str,
    amount_cents: i64,
    account_kind: &'static str,
    to_account_kind: Option<&'static str>,
    category_idx: usize,
    merchant_idx: Option<usize>,
    special: &'static [(u32, &'static str)],
}

const PLANS: [PlanSpec; 8] = [
    PlanSpec {
        kind: "installment",
        label: "手机分期",
        months_ago: 7,
        total_occurrences: Some(12),
        status: "active",
        amount_cents: 250_000,
        account_kind: "credit",
        to_account_kind: None,
        category_idx: 2,
        merchant_idx: Some(0),
        special: &[],
    },
    PlanSpec {
        kind: "installment",
        label: "笔记本分期",
        months_ago: 4,
        total_occurrences: Some(12),
        status: "active",
        amount_cents: 45_000,
        account_kind: "credit",
        to_account_kind: None,
        category_idx: 6,
        merchant_idx: Some(3),
        special: &[],
    },
    PlanSpec {
        kind: "installment",
        label: "健身卡分期",
        months_ago: 2,
        total_occurrences: Some(6),
        status: "active",
        amount_cents: 100_000,
        account_kind: "bank",
        to_account_kind: None,
        category_idx: 4,
        merchant_idx: None,
        special: &[],
    },
    PlanSpec {
        kind: "subscription",
        label: "视频会员",
        months_ago: 5,
        total_occurrences: None,
        status: "active",
        amount_cents: 2_500,
        account_kind: "ewallet",
        to_account_kind: None,
        category_idx: 0,
        merchant_idx: Some(9),
        special: &[(2, "failed")],
    },
    PlanSpec {
        kind: "subscription",
        label: "云存储年费",
        months_ago: 2,
        total_occurrences: None,
        status: "active",
        amount_cents: 30_000,
        account_kind: "ewallet",
        to_account_kind: None,
        category_idx: 8,
        merchant_idx: None,
        special: &[],
    },
    PlanSpec {
        kind: "subscription",
        label: "健身房月卡",
        months_ago: 3,
        total_occurrences: None,
        status: "active",
        amount_cents: 16_000,
        account_kind: "bank",
        to_account_kind: None,
        category_idx: 3,
        merchant_idx: Some(6),
        special: &[(1, "failed"), (2, "cancelled")],
    },
    PlanSpec {
        kind: "scheduled_transfer",
        label: "基金定投",
        months_ago: 4,
        total_occurrences: None,
        status: "active",
        amount_cents: 200_000,
        account_kind: "bank",
        to_account_kind: Some("investment"),
        category_idx: 0,
        merchant_idx: None,
        special: &[],
    },
    PlanSpec {
        kind: "scheduled_transfer",
        label: "房租划转",
        months_ago: 6,
        total_occurrences: None,
        status: "paused",
        amount_cents: 350_000,
        account_kind: "cash",
        to_account_kind: Some("bank"),
        category_idx: 0,
        merchant_idx: None,
        special: &[],
    },
];

struct AccountPicker {
    by_kind: Vec<(&'static str, String)>,
}

impl AccountPicker {
    fn new(accounts: &[AccountRow]) -> Self {
        let mut by_kind: Vec<(&'static str, String)> = Vec::new();
        for a in accounts {
            if a.ccy != "CNY" {
                continue;
            }
            if !by_kind.iter().any(|(k, _)| *k == a.atype) {
                by_kind.push((a.atype, a.id.clone()));
            }
        }
        AccountPicker { by_kind }
    }

    fn pick(&self, kind: &str) -> Option<&str> {
        self.by_kind
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, id)| id.as_str())
    }
}

enum OccurrenceState {
    Completed,
    Skipped(&'static str),
    Pending,
}

pub(crate) fn insert_scheduled(
    conn: &Connection,
    expense_pool: &[String],
    top_merchants: &[String],
    accounts: &[AccountRow],
    end_date: NaiveDate,
    counts: &mut GenCounts,
) -> Result<u64, String> {
    let picker = AccountPicker::new(accounts);
    let active_horizon = end_date + Months::new(PENDING_HORIZON_MONTHS);
    let paused_horizon = end_date
        .with_day(1)
        .map(|d| d - Duration::days(1))
        .unwrap_or(end_date);

    let plan_sql = "INSERT INTO scheduled_transactions (id,kind,status,account_id,category_id,\
         amount_cents,currency_code,recurrence_type,recurrence_interval,recurrence_day,\
         start_date,note,created_at,updated_at,version,device_id,is_deleted)\
         VALUES (?1,?2,?3,?4,?5,?6,'CNY','monthly',1,?7,?8,?9,?10,?10,1,?11,0)";
    let mut plan_stmt = conn.prepare(plan_sql).map_err(|e| e.to_string())?;
    let installment_sql = "INSERT INTO installment_plans (scheduled_transaction_id,merchant_id,\
         total_amount_cents,total_occurrences) VALUES (?1,?2,?3,?4)";
    let mut installment_stmt = conn.prepare(installment_sql).map_err(|e| e.to_string())?;
    let subscription_sql = "INSERT INTO subscription_plans (scheduled_transaction_id,merchant_id,\
         policy_id) VALUES (?1,?2,NULL)";
    let mut subscription_stmt = conn.prepare(subscription_sql).map_err(|e| e.to_string())?;
    let transfer_sql = "INSERT INTO scheduled_transfer_plans (scheduled_transaction_id,\
         to_account_id,total_occurrences) VALUES (?1,?2,NULL)";
    let mut transfer_stmt = conn.prepare(transfer_sql).map_err(|e| e.to_string())?;
    let occurrence_sql = "INSERT INTO scheduled_transaction_occurrences (id,\
         scheduled_transaction_id,scheduled_date,status,transaction_id,amount_cents,\
         created_at,updated_at,version,device_id,is_deleted)\
         VALUES (?1,?2,?3,?4,?5,?6,?7,?7,1,?8,0)";
    let mut occurrence_stmt = conn.prepare(occurrence_sql).map_err(|e| e.to_string())?;
    let txn_sql = "INSERT INTO transactions (id,kind,amount_cents,currency_code,\
         amount_native_cents,account_id,to_account_id,category_id,merchant_id,\
         refund_of_transaction_id,note,dedup_hash,idempotency_key,date,created_at,updated_at,\
         version,device_id,is_deleted,policy_id)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL,?10,NULL,NULL,?11,?12,?12,1,?13,0,NULL)";
    let mut txn_stmt = conn.prepare(txn_sql).map_err(|e| e.to_string())?;

    let mut occurrence_seq = 0u64;
    let mut txn_seq = 0u64;
    let mut occurrence_txns = 0u64;
    for (plan_seq, spec) in PLANS.iter().enumerate() {
        let account = picker.pick(spec.account_kind);
        let Some(account_id) = account else {
            return Err(format!(
                "缺少类型为 {} 的 CNY 账户，无法生成计划 {}",
                spec.account_kind, spec.label
            ));
        };
        let to_account = match spec.to_account_kind {
            Some(kind) => {
                let picked = picker
                    .pick(kind)
                    .filter(|picked| *picked != account_id)
                    .or_else(|| {
                        accounts
                            .iter()
                            .find(|a| a.ccy == "CNY" && a.id != account_id)
                            .map(|a| a.id.as_str())
                    });
                picked
                    .ok_or_else(|| format!("缺少可作转入账户的 CNY 账户：{kind}"))?
                    .to_string()
            }
            None => String::new(),
        };
        let category = if spec.kind == "scheduled_transfer" {
            None
        } else {
            expense_pool.get(spec.category_idx).cloned()
        };
        let merchant = spec
            .merchant_idx
            .and_then(|i| top_merchants.get(i))
            .cloned();

        let start = end_date
            .with_day(10)
            .and_then(|d| d.checked_sub_months(Months::new(spec.months_ago)))
            .ok_or_else(|| {
                format!(
                    "计划起点越界：{} months_ago={}",
                    spec.label, spec.months_ago
                )
            })?;
        let start_millis = date_millis(&start);
        let plan_id = time_ordered_id("scheduled_plans", plan_seq as u64, start_millis);
        let plan_stamp = format!("{start}T08:00:00Z");

        plan_stmt
            .execute(rusqlite::params![
                plan_id,
                spec.kind,
                spec.status,
                account_id,
                category,
                spec.amount_cents,
                10,
                start.to_string(),
                spec.label,
                plan_stamp,
                DEVICE_ID,
            ])
            .map_err(|e| e.to_string())?;

        match spec.kind {
            "installment" => {
                let total = spec.total_occurrences.unwrap_or(1);
                installment_stmt
                    .execute(rusqlite::params![
                        plan_id,
                        merchant.clone(),
                        spec.amount_cents * i64::from(total),
                        total,
                    ])
                    .map_err(|e| e.to_string())?;
            }
            "subscription" => {
                subscription_stmt
                    .execute(rusqlite::params![plan_id, merchant.clone()])
                    .map_err(|e| e.to_string())?;
            }
            _ => {
                transfer_stmt
                    .execute(rusqlite::params![plan_id, to_account])
                    .map_err(|e| e.to_string())?;
            }
        }

        let horizon = if spec.status == "paused" {
            paused_horizon
        } else {
            active_horizon
        };
        let mut k = 0u32;
        loop {
            if spec.total_occurrences.is_some_and(|total| k >= total) {
                break;
            }
            let date = start
                .checked_add_months(Months::new(k))
                .ok_or_else(|| format!("期次日期越界：{} k={k}", spec.label))?;
            if date > horizon {
                break;
            }
            let state = match spec.special.iter().find(|(sk, _)| *sk == k) {
                Some((_, status)) => OccurrenceState::Skipped(status),
                None if date <= end_date => OccurrenceState::Completed,
                None => OccurrenceState::Pending,
            };
            let occ_id = time_ordered_id("occurrences", occurrence_seq, date_millis(&date));
            occurrence_seq += 1;
            let occ_stamp = format!("{date}T{OCCURRENCE_HOUR}");
            let txn_id = match state {
                OccurrenceState::Completed => {
                    let txn_id = time_ordered_id("transactions", txn_seq, date_millis(&date));
                    txn_seq += 1;
                    occurrence_txns += 1;
                    let (kind, to, cat) = if spec.kind == "scheduled_transfer" {
                        ("transfer", Some(to_account.clone()), None)
                    } else {
                        ("expense", None, category.clone())
                    };
                    txn_stmt
                        .execute(rusqlite::params![
                            txn_id,
                            kind,
                            spec.amount_cents,
                            "CNY",
                            spec.amount_cents,
                            account_id,
                            to,
                            cat,
                            merchant.clone(),
                            spec.label,
                            date.to_string(),
                            occ_stamp,
                            DEVICE_ID,
                        ])
                        .map_err(|e| e.to_string())?;
                    Some(txn_id)
                }
                OccurrenceState::Skipped(_) => None,
                OccurrenceState::Pending => None,
            };
            let status = match state {
                OccurrenceState::Completed => "completed",
                OccurrenceState::Skipped(status) => status,
                OccurrenceState::Pending => "pending",
            };
            occurrence_stmt
                .execute(rusqlite::params![
                    occ_id,
                    plan_id,
                    date.to_string(),
                    status,
                    txn_id,
                    spec.amount_cents,
                    occ_stamp,
                    DEVICE_ID,
                ])
                .map_err(|e| e.to_string())?;
            k += 1;
        }
    }

    counts.scheduled_plans = PLANS.len();
    counts.scheduled_occurrence_transactions = occurrence_txns as usize;
    counts.scheduled_occurrences = occurrence_seq as usize;
    Ok(occurrence_txns)
}
