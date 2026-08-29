//! `scheduled_transactions` 测试共享脚手架：仅限本测试目录（`scheduled_transactions::tests`）内部使用。

use super::super::*;
use rusqlite::Connection;
use rusqlite::params;

pub(crate) fn setup_db() -> Connection {
    let mut conn = crate::db::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn
}

pub(crate) fn insert_account(conn: &Connection, id: &str, currency: &str) {
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,'cash',?3,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id, id, currency],
    )
    .unwrap();
}

pub(crate) fn insert_rate(conn: &Connection, base: &str, quote: &str, rate: f64) {
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,updated_at,version,device_id) \
         VALUES ('er-1',?1,?2,?3,'2026-02-01T00:00:00Z','2026-02-01T00:00:00Z',1,'test')",
        params![base, quote, rate],
    )
    .unwrap();
}

/// 创建订阅计划（无上限，预生成窗口期次），返回计划 id。
pub(crate) fn create_subscription(
    conn: &Connection,
    account_id: &str,
    currency: &str,
    amount_cents: i64,
    note: Option<&str>,
) -> String {
    create_subscription_cycle(
        conn,
        account_id,
        currency,
        amount_cents,
        RecurrenceType::Monthly,
        1,
        note,
    )
}

/// 创建指定周期类型与间隔的订阅计划，返回计划 id。
pub(crate) fn create_subscription_cycle(
    conn: &Connection,
    account_id: &str,
    currency: &str,
    amount_cents: i64,
    recurrence_type: RecurrenceType,
    recurrence_interval: i64,
    note: Option<&str>,
) -> String {
    create_plan(
        conn,
        CreateScheduledInput {
            kind: ScheduledKind::Subscription,
            account_id: account_id.into(),
            category_id: None,
            amount_cents,
            currency_code: currency.into(),
            recurrence_type,
            recurrence_interval,
            recurrence_day: None,
            start_date: "2026-01-15".into(),
            note: note.map(String::from),
            merchant_id: None,
            total_amount_cents: None,
            total_occurrences: None,
            to_account_id: None,
        },
    )
    .unwrap()
}

/// 创建定时转账计划（固定 3 期），返回计划 id。
pub(crate) fn create_transfer_plan(
    conn: &Connection,
    from: &str,
    to: &str,
    amount_cents: i64,
) -> String {
    create_plan(
        conn,
        CreateScheduledInput {
            kind: ScheduledKind::ScheduledTransfer,
            account_id: from.into(),
            category_id: None,
            amount_cents,
            currency_code: "CNY".into(),
            recurrence_type: RecurrenceType::Monthly,
            recurrence_interval: 1,
            recurrence_day: None,
            start_date: "2026-01-15".into(),
            note: None,
            merchant_id: None,
            total_amount_cents: None,
            total_occurrences: Some(3),
            to_account_id: Some(to.into()),
        },
    )
    .unwrap()
}

/// 创建分期计划（总额/期数），返回计划 id。
pub(crate) fn create_installment(
    conn: &Connection,
    account_id: &str,
    total_cents: i64,
    total_occ: i64,
) -> String {
    create_plan(
        conn,
        CreateScheduledInput {
            kind: ScheduledKind::Installment,
            account_id: account_id.into(),
            category_id: None,
            amount_cents: total_cents / total_occ,
            currency_code: "CNY".into(),
            recurrence_type: RecurrenceType::Monthly,
            recurrence_interval: 1,
            recurrence_day: None,
            start_date: "2026-01-15".into(),
            note: None,
            merchant_id: None,
            total_amount_cents: Some(total_cents),
            total_occurrences: Some(total_occ),
            to_account_id: None,
        },
    )
    .unwrap()
}

/// 取计划的第一条 pending 期次 id（计划创建时已预生成）。
pub(crate) fn first_pending_occurrence(conn: &Connection, plan_id: &str) -> String {
    get_plan_detail(conn, plan_id)
        .unwrap()
        .pending_occurrences
        .into_iter()
        .next()
        .expect("计划应已有 pending 期次")
        .id
}

/// 读回交易的落库字段（供断言与 writer 列映射一致）。
pub(crate) struct TxnRow {
    pub(crate) kind: String,
    pub(crate) amount_cents: i64,
    pub(crate) currency_code: String,
    pub(crate) amount_native_cents: i64,
    pub(crate) account_id: String,
    pub(crate) to_account_id: Option<String>,
    pub(crate) category_id: Option<String>,
    pub(crate) merchant_id: Option<String>,
    pub(crate) refund_of_transaction_id: Option<String>,
    pub(crate) note: Option<String>,
    pub(crate) date: String,
}

pub(crate) fn read_txn(conn: &Connection, id: &str) -> TxnRow {
    conn.query_row(
        "SELECT kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,merchant_id,refund_of_transaction_id,note,date FROM transactions WHERE id=?1",
        params![id],
        |r| {
            Ok(TxnRow {
                kind: r.get(0)?,
                amount_cents: r.get(1)?,
                currency_code: r.get(2)?,
                amount_native_cents: r.get(3)?,
                account_id: r.get(4)?,
                to_account_id: r.get(5)?,
                category_id: r.get(6)?,
                merchant_id: r.get(7)?,
                refund_of_transaction_id: r.get(8)?,
                note: r.get(9)?,
                date: r.get(10)?,
            })
        },
    )
    .unwrap()
}

/// 期次状态 + 回填的交易 id。
pub(crate) fn occurrence_status(conn: &Connection, occ_id: &str) -> (String, Option<String>) {
    conn.query_row(
        "SELECT status,transaction_id FROM scheduled_transaction_occurrences WHERE id=?1",
        params![occ_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}
