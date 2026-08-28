//! `scheduled_transactions::engine` 的单元测试（issue #59 / spec #52）。
//!
//! 断言定时引擎的落库路径已收敛到 Writer 接缝（`writer::normalize` + `insert_row`）：
//! - 回归：非默认币种定时交易的 `amount_native_cents` 经 Amount 接缝折算，
//!   不再把原始金额当作本位币金额落库（故事 3/17/23）；
//! - 既有行为不变：expense / transfer 映射与计划/期次状态流转逐项锁定。
//!
//! 全部基于内存库，走 `engine` 公开 API（create_plan / execute_occurrence 等）。

use rusqlite::Connection;
use rusqlite::params;

use super::*;

fn setup_db() -> Connection {
    let mut conn = crate::db::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn
}

fn insert_account(conn: &Connection, id: &str, currency: &str) {
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,'cash',?3,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id, id, currency],
    )
    .unwrap();
}

fn insert_rate(conn: &Connection, base: &str, quote: &str, rate: f64) {
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,updated_at,version,device_id) \
         VALUES ('er-1',?1,?2,?3,'2026-02-01T00:00:00Z','2026-02-01T00:00:00Z',1,'test')",
        params![base, quote, rate],
    )
    .unwrap();
}

/// 创建订阅计划（无上限，预生成窗口期次），返回计划 id。
fn create_subscription(
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
fn create_subscription_cycle(
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
            counterparty: Some("Netflix".into()),
            total_amount_cents: None,
            total_occurrences: None,
            to_account_id: None,
        },
    )
    .unwrap()
}

/// 创建定时转账计划（固定 3 期），返回计划 id。
fn create_transfer_plan(conn: &Connection, from: &str, to: &str, amount_cents: i64) -> String {
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
            counterparty: None,
            total_amount_cents: None,
            total_occurrences: Some(3),
            to_account_id: Some(to.into()),
        },
    )
    .unwrap()
}

/// 创建分期计划（总额/期数），返回计划 id。
fn create_installment(
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
            counterparty: Some("京东白条".into()),
            total_amount_cents: Some(total_cents),
            total_occurrences: Some(total_occ),
            to_account_id: None,
        },
    )
    .unwrap()
}

/// 取计划的第一条 pending 期次 id（计划创建时已预生成）。
fn first_pending_occurrence(conn: &Connection, plan_id: &str) -> String {
    get_plan_detail(conn, plan_id)
        .unwrap()
        .pending_occurrences
        .into_iter()
        .next()
        .expect("计划应已有 pending 期次")
        .id
}

/// 读回交易的落库字段（供断言与 writer 列映射一致）。
struct TxnRow {
    kind: String,
    amount_cents: i64,
    currency_code: String,
    amount_native_cents: i64,
    account_id: String,
    to_account_id: Option<String>,
    category_id: Option<String>,
    refund_of_transaction_id: Option<String>,
    note: Option<String>,
    date: String,
}

fn read_txn(conn: &Connection, id: &str) -> TxnRow {
    conn.query_row(
        "SELECT kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date FROM transactions WHERE id=?1",
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
                refund_of_transaction_id: r.get(7)?,
                note: r.get(8)?,
                date: r.get(9)?,
            })
        },
    )
    .unwrap()
}

/// 期次状态 + 回填的交易 id。
fn occurrence_status(conn: &Connection, occ_id: &str) -> (String, Option<String>) {
    conn.query_row(
        "SELECT status,transaction_id FROM scheduled_transaction_occurrences WHERE id=?1",
        params![occ_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}

// ---------------------------------------------------------------------------
// 回归：非默认币种的定时交易折算（issue #59 核心 bug）
// ---------------------------------------------------------------------------

/// 非默认币种（USD）订阅计划执行后，`amount_native_cents` 按汇率折算到本位币，
/// 而不是把原始金额当作本位币金额落库（修复前的 bug 行为）。
#[test]
fn execute_occurrence_converts_non_default_currency_to_native() {
    let conn = setup_db();
    insert_account(&conn, "acc-usd", "USD");
    insert_rate(&conn, "USD", "CNY", 7.2);
    let plan_id = create_subscription(&conn, "acc-usd", "USD", 10000, Some("国际订阅"));
    let occ_id = first_pending_occurrence(&conn, &plan_id);

    let txn_id = execute_occurrence(&conn, &occ_id).unwrap();

    let txn = read_txn(&conn, &txn_id);
    assert_eq!(txn.kind, "expense");
    assert_eq!(txn.amount_cents, 10000, "原始币种金额保留");
    assert_eq!(txn.currency_code, "USD");
    assert_eq!(
        txn.amount_native_cents, 72000,
        "本位币金额应经 convert_to_native 折算"
    );
    assert_eq!(txn.account_id, "acc-usd");
    assert_eq!(txn.to_account_id, None);
    assert_eq!(txn.note.as_deref(), Some("国际订阅"));
    assert_eq!(txn.date, "2026-01-15");
}

/// 非默认币种且无汇率 → 执行报错，不静默 1:1 写错本位币金额；
/// 且期次保持 pending（normalize 在 CAS 锁定前完成），可重试、不滞留 processing。
#[test]
fn execute_occurrence_errors_without_rate_for_non_default_currency() {
    let conn = setup_db();
    insert_account(&conn, "acc-jpy", "JPY");
    let plan_id = create_subscription(&conn, "acc-jpy", "JPY", 10000, None);
    let occ_id = first_pending_occurrence(&conn, &plan_id);

    let err = execute_occurrence(&conn, &occ_id).unwrap_err();
    assert!(err.to_string().contains("汇率"), "实际: {err}");

    // 业务错误发生在 CAS 锁定之前：期次必须保持 pending，等待补录汇率后重试
    let (status, backfilled) = occurrence_status(&conn, &occ_id);
    assert_eq!(status, "pending", "期次不应滞留 processing");
    assert_eq!(backfilled, None, "失败不应回填交易 id");
    // 补录汇率后同一期次可重试成功
    insert_rate(&conn, "JPY", "CNY", 0.05);
    let txn_id = execute_occurrence(&conn, &occ_id).unwrap();
    let txn = read_txn(&conn, &txn_id);
    assert_eq!(txn.amount_native_cents, 500);
}

/// 默认币种（CNY）定时交易本位币与原始金额 1:1（MVP 口径不变）。
#[test]
fn execute_occurrence_default_currency_stays_1_1() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let plan_id = create_subscription(&conn, "acc", "CNY", 6600, Some("视频会员"));
    let occ_id = first_pending_occurrence(&conn, &plan_id);

    let txn_id = execute_occurrence(&conn, &occ_id).unwrap();
    let txn = read_txn(&conn, &txn_id);
    assert_eq!(txn.amount_cents, 6600);
    assert_eq!(txn.amount_native_cents, 6600, "默认币种应 1:1");
}

// ---------------------------------------------------------------------------
// 既有行为不变：expense 映射
// ---------------------------------------------------------------------------

/// 订阅计划 → 支出交易：kind=expense、账户/备注/日期来自计划与期次；
/// 期次置为 completed 并回填 transaction_id。
#[test]
fn execute_subscription_maps_expense_and_completes_occurrence() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let plan_id = create_subscription(&conn, "acc", "CNY", 3000, Some("云服务"));
    let occ_id = first_pending_occurrence(&conn, &plan_id);

    let txn_id = execute_occurrence(&conn, &occ_id).unwrap();

    let txn = read_txn(&conn, &txn_id);
    assert_eq!(txn.kind, "expense");
    assert_eq!(txn.account_id, "acc");
    assert_eq!(txn.category_id, None);
    assert_eq!(txn.note.as_deref(), Some("云服务"));
    assert_eq!(txn.date, "2026-01-15");
    assert_eq!(txn.refund_of_transaction_id, None); // 定时支出不关联退款

    let (status, backfilled) = occurrence_status(&conn, &occ_id);
    assert_eq!(status, "completed", "期次应流转为 completed");
    assert_eq!(
        backfilled.as_deref(),
        Some(txn_id.as_str()),
        "期次应回填交易 id"
    );
}

/// 订阅计划带分类时，支出交易继承计划分类。
#[test]
fn execute_subscription_inherits_category() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let plan_id = create_plan(
        &conn,
        CreateScheduledInput {
            kind: ScheduledKind::Subscription,
            account_id: "acc".into(),
            category_id: Some("95d6dc66-12c4-5f2b-bf9b-1d439a9c8100".into()), // 餐饮
            amount_cents: 3000,
            currency_code: "CNY".into(),
            recurrence_type: RecurrenceType::Monthly,
            recurrence_interval: 1,
            recurrence_day: None,
            start_date: "2026-01-15".into(),
            note: None,
            counterparty: Some("Netflix".into()),
            total_amount_cents: None,
            total_occurrences: None,
            to_account_id: None,
        },
    )
    .unwrap();
    let occ_id = first_pending_occurrence(&conn, &plan_id);

    let txn_id = execute_occurrence(&conn, &occ_id).unwrap();
    let txn = read_txn(&conn, &txn_id);
    assert_eq!(txn.kind, "expense");
    assert_eq!(
        txn.category_id.as_deref(),
        Some("95d6dc66-12c4-5f2b-bf9b-1d439a9c8100"),
        "支出交易应继承计划分类"
    );
}

// ---------------------------------------------------------------------------
// 既有行为不变：transfer 映射
// ---------------------------------------------------------------------------

/// 定时转账计划 → 转账交易：kind=transfer、account_id 转出、to_account_id 转入。
#[test]
fn execute_transfer_maps_out_and_in_accounts() {
    let conn = setup_db();
    insert_account(&conn, "acc-a", "CNY");
    insert_account(&conn, "acc-b", "CNY");
    let plan_id = create_transfer_plan(&conn, "acc-a", "acc-b", 50000);
    let occ_id = first_pending_occurrence(&conn, &plan_id);

    let txn_id = execute_occurrence(&conn, &occ_id).unwrap();

    let txn = read_txn(&conn, &txn_id);
    assert_eq!(txn.kind, "transfer");
    assert_eq!(txn.account_id, "acc-a", "转出账户");
    assert_eq!(txn.to_account_id.as_deref(), Some("acc-b"), "转入账户");
    assert_eq!(txn.category_id, None);
    assert_eq!(txn.amount_native_cents, 50000);
}

// ---------------------------------------------------------------------------
// 既有行为不变：计划/期次状态流转
// ---------------------------------------------------------------------------

/// 分期计划按期执行：各期金额 = 总额/期数（末期为余数尾差），
/// 全部执行完毕后计划状态置为 completed。
#[test]
fn execute_installments_all_marks_plan_completed() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    // 3100 / 3：前两期 1033，末期 1034（尾差并入末期）
    let plan_id = create_installment(&conn, "acc", 3100, 3);
    let occ_ids: Vec<String> = get_plan_detail(&conn, &plan_id)
        .unwrap()
        .pending_occurrences
        .into_iter()
        .map(|o| o.id)
        .collect();
    assert_eq!(occ_ids.len(), 3);

    let mut txns = Vec::new();
    for occ_id in &occ_ids {
        txns.push(execute_occurrence(&conn, occ_id).unwrap());
    }

    // 各期金额与分期计划一致，交易 id 互异
    let amounts: Vec<i64> = txns
        .iter()
        .map(|id| read_txn(&conn, id).amount_cents)
        .collect();
    assert_eq!(amounts, vec![1033, 1033, 1034]);
    let mut distinct = txns.clone();
    distinct.dedup();
    assert_eq!(distinct.len(), 3, "每期应落独立交易");

    // 计划 → completed；期次全部 completed
    let plan_status: String = conn
        .query_row(
            "SELECT status FROM scheduled_transactions WHERE id=?1",
            params![plan_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(plan_status, "completed", "全部期次完成应置计划为 completed");
    for occ_id in &occ_ids {
        assert_eq!(occurrence_status(&conn, occ_id).0, "completed");
    }
}

/// 已暂停的计划执行期次 → 报错（状态流转不变）。
#[test]
fn execute_occurrence_rejects_paused_plan() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let plan_id = create_subscription(&conn, "acc", "CNY", 3000, None);
    update_plan_status(&conn, &plan_id, ScheduledStatus::Paused).unwrap();
    let occ_id = first_pending_occurrence(&conn, &plan_id);

    let err = execute_occurrence(&conn, &occ_id).unwrap_err();
    assert_eq!(err.to_string(), "参数错误: 关联计划未处于活跃状态");
}

// ---------------------------------------------------------------------------
// 订阅花费——实际花费口径（issue #160，ADR-0023 决策二）
// ---------------------------------------------------------------------------

use crate::scheduled_transactions::query_subscription_spend;

/// 执行计划前 N 条 pending 期次（scheduled_date 升序），返回生成的交易日期。
fn execute_first_n_occurrences(conn: &Connection, plan_id: &str, n: usize) -> Vec<String> {
    let occ_ids: Vec<String> = get_plan_detail(conn, plan_id)
        .unwrap()
        .pending_occurrences
        .into_iter()
        .take(n)
        .map(|o| o.id)
        .collect();
    occ_ids
        .iter()
        .map(|id| read_txn(conn, &execute_occurrence(conn, id).unwrap()).date)
        .collect()
}

fn date(s: &str) -> chrono::NaiveDate {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn month_cents(overview: &SubscriptionSpendOverview, month: &str) -> i64 {
    overview
        .months
        .iter()
        .find(|m| m.month == month)
        .unwrap_or_else(|| panic!("12 个月序列应包含 {month}"))
        .native_cents
}

/// 实际花费按期次流水逐月忠实统计（本位币），非扣款月补 0；不摊销。
#[test]
fn subscription_spend_aggregates_by_calendar_month() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let plan_id = create_subscription(&conn, "acc", "CNY", 3000, Some("视频会员"));
    // 2026-01-15 起月付，执行前两期 → 2026-01 / 2026-02 各一笔 3000
    execute_first_n_occurrences(&conn, &plan_id, 2);

    let overview = query_subscription_spend(&conn, date("2026-03-20")).unwrap();
    assert_eq!(overview.native_currency, "CNY");
    assert_eq!(month_cents(&overview, "2026-01"), 3000);
    assert_eq!(month_cents(&overview, "2026-02"), 3000);
    assert_eq!(month_cents(&overview, "2026-03"), 0, "未扣款月应为 0");
    assert_eq!(overview.this_month_native_cents, 0, "本月（2026-03）无扣款");
    assert_eq!(overview.this_year_native_cents, 6000);
    assert_eq!(overview.months.len(), 12, "固定 12 个月槽位");
    assert_eq!(overview.months[0].month, "2025-04", "旧→新，含当月");
    assert_eq!(overview.months[11].month, "2026-03");
}

/// 年付订阅不摊销：扣款月全额计入，其余月份为 0。
#[test]
fn subscription_spend_yearly_not_amortized() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let plan_id = create_plan(
        &conn,
        CreateScheduledInput {
            kind: ScheduledKind::Subscription,
            account_id: "acc".into(),
            category_id: None,
            amount_cents: 34800,
            currency_code: "CNY".into(),
            recurrence_type: RecurrenceType::Yearly,
            recurrence_interval: 1,
            recurrence_day: None,
            start_date: "2026-01-10".into(),
            note: Some("云存储年费".into()),
            counterparty: None,
            total_amount_cents: None,
            total_occurrences: None,
            to_account_id: None,
        },
    )
    .unwrap();
    execute_first_n_occurrences(&conn, &plan_id, 1);

    let overview = query_subscription_spend(&conn, date("2026-03-20")).unwrap();
    assert_eq!(month_cents(&overview, "2026-01"), 34800, "扣款月全额计入");
    assert_eq!(month_cents(&overview, "2026-02"), 0, "不摊销");
    assert_eq!(month_cents(&overview, "2026-03"), 0, "不摊销");
    assert_eq!(overview.this_month_native_cents, 0);
    assert_eq!(overview.this_year_native_cents, 34800);
}

/// 计划取消/暂停不影响历史实际花费；非订阅计划（分期/定时转账）不计入。
#[test]
fn subscription_spend_keeps_cancelled_history_and_excludes_other_kinds() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let sub_id = create_subscription(&conn, "acc", "CNY", 3000, Some("视频会员"));
    execute_first_n_occurrences(&conn, &sub_id, 2);
    update_plan_status(&conn, &sub_id, ScheduledStatus::Cancelled).unwrap();

    // 干扰项：分期与定时转账各执行一期，不应计入订阅花费
    let inst_id = create_installment(&conn, "acc", 3100, 3);
    execute_first_n_occurrences(&conn, &inst_id, 1);
    let transfer_id = {
        insert_account(&conn, "acc2", "CNY");
        create_transfer_plan(&conn, "acc", "acc2", 50000)
    };
    execute_first_n_occurrences(&conn, &transfer_id, 1);

    let overview = query_subscription_spend(&conn, date("2026-03-20")).unwrap();
    assert_eq!(month_cents(&overview, "2026-01"), 3000, "取消后历史保留");
    assert_eq!(month_cents(&overview, "2026-02"), 3000, "取消后历史保留");
    assert_eq!(overview.this_year_native_cents, 6000, "分期/转账不计入");

    // 逐订阅行：取消计划仍在行内，行内本月/本年口径正确
    assert_eq!(overview.rows.len(), 1, "只统计订阅计划");
    let row = &overview.rows[0];
    assert_eq!(row.plan_id, sub_id);
    assert_eq!(row.status, "cancelled");
    assert_eq!(row.this_month_native_cents, 0);
    assert_eq!(row.this_year_native_cents, 6000);
}

/// 推算成本（issue #161）：各周期系数折算正确（月 ×1、年 ÷12、周 ×52÷12、日 ×30），
/// recurrence_interval > 1 时按间隔均摊；折算年成本 = 折算月成本 × 12。
#[test]
fn subscription_projected_spend_coefficients() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    create_subscription_cycle(
        &conn,
        "acc",
        "CNY",
        3000,
        RecurrenceType::Monthly,
        1,
        Some("月付"),
    );
    create_subscription_cycle(
        &conn,
        "acc",
        "CNY",
        34800,
        RecurrenceType::Yearly,
        1,
        Some("年付"),
    );
    create_subscription_cycle(
        &conn,
        "acc",
        "CNY",
        5200,
        RecurrenceType::Weekly,
        1,
        Some("周付"),
    );
    create_subscription_cycle(
        &conn,
        "acc",
        "CNY",
        300,
        RecurrenceType::Daily,
        1,
        Some("日付"),
    );
    create_subscription_cycle(
        &conn,
        "acc",
        "CNY",
        3000,
        RecurrenceType::Monthly,
        3,
        Some("每三月"),
    );

    let overview = query_subscription_spend(&conn, date("2026-03-20")).unwrap();
    // 3000×1 + 34800÷12 + 5200×52÷12 + 300×30 + 3000÷3 = 3000 + 2900 + 22533 + 9000 + 1000
    assert_eq!(overview.projected_month_native_cents, 38433);
    assert_eq!(
        overview.projected_year_native_cents,
        38433 * 12,
        "折算年成本 = 折算月成本 × 12"
    );
}

/// 推算成本只统计 active 计划（暂停/取消不计入），且不看执行情况（未执行也计入）。
#[test]
fn subscription_projected_spend_counts_only_active_plans() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    create_subscription(&conn, "acc", "CNY", 3000, Some("进行中"));
    let paused = create_subscription(&conn, "acc", "CNY", 5000, Some("已暂停"));
    update_plan_status(&conn, &paused, ScheduledStatus::Paused).unwrap();
    let cancelled = create_subscription(&conn, "acc", "CNY", 7000, Some("已取消"));
    update_plan_status(&conn, &cancelled, ScheduledStatus::Cancelled).unwrap();

    let overview = query_subscription_spend(&conn, date("2026-03-20")).unwrap();
    assert_eq!(
        overview.projected_month_native_cents, 3000,
        "暂停/取消不计入，未执行也计入"
    );
    assert_eq!(overview.projected_year_native_cents, 36000);
    // 推算口径不影响实际口径：均未执行，实际花费为 0
    assert_eq!(overview.this_month_native_cents, 0);
    assert_eq!(overview.this_year_native_cents, 0);
}

/// 推算成本在计划币种上折算本位币；缺汇率时报错上抛，不静默混算。
#[test]
fn subscription_projected_spend_converts_and_requires_rate() {
    let conn = setup_db();
    insert_account(&conn, "acc-usd", "USD");
    insert_rate(&conn, "USD", "CNY", 7.2);
    create_subscription(&conn, "acc-usd", "USD", 10000, Some("国际订阅"));

    let overview = query_subscription_spend(&conn, date("2026-03-20")).unwrap();
    assert_eq!(
        overview.projected_month_native_cents, 72000,
        "10000 × 7.2 折算本位币"
    );

    conn.execute("DELETE FROM exchange_rates", params![])
        .unwrap();
    let err = query_subscription_spend(&conn, date("2026-03-20")).unwrap_err();
    assert!(err.to_string().contains("汇率"), "缺汇率应报错上抛: {err}");
}

/// 非默认币种订阅按流水的本位币金额（落库时折算）计入，不二次折算。
#[test]
fn subscription_spend_uses_native_amounts_from_transactions() {
    let conn = setup_db();
    insert_account(&conn, "acc-usd", "USD");
    insert_rate(&conn, "USD", "CNY", 7.2);
    let plan_id = create_subscription(&conn, "acc-usd", "USD", 10000, Some("国际订阅"));
    execute_first_n_occurrences(&conn, &plan_id, 1);

    let overview = query_subscription_spend(&conn, date("2026-03-20")).unwrap();
    assert_eq!(
        month_cents(&overview, "2026-01"),
        72000,
        "应取流水 amount_native_cents（10000 × 7.2）"
    );
    let row = &overview.rows[0];
    assert_eq!(row.amount_cents, 10000, "行内原始金额保持计划币种");
    assert_eq!(row.currency_code, "USD");
    assert_eq!(row.this_year_native_cents, 72000);
}

// ---------------------------------------------------------------------------
// 订阅编辑——仅非金额字段（issue #162，ADR-0023 决策三）
// ---------------------------------------------------------------------------

/// 金额哨兵边界：请求携带 `amount_cents` / `total_amount_cents`（含显式 null）
/// 一律显式拒绝，且拒绝后计划字段不被改动。
#[test]
fn update_subscription_rejects_amount_field_including_explicit_null() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let plan_id = create_subscription(&conn, "acc", "CNY", 3000, Some("视频会员"));

    let payloads = [
        r#"{"id":"{id}","account_id":"acc","note":"x","amount_cents":5000}"#,
        r#"{"id":"{id}","account_id":"acc","note":"x","amount_cents":null}"#,
        r#"{"id":"{id}","account_id":"acc","note":"x","total_amount_cents":null}"#,
    ];
    for payload in payloads {
        let json = payload.replace("{id}", &plan_id);
        let input: UpdateSubscriptionInput = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("反序列化应成功（拒绝发生在领域层）: {e}"));
        let err =
            update_subscription(&conn, input).expect_err("携带金额字段的编辑请求应被显式拒绝");
        assert!(
            err.to_string().contains("改价 = 取消旧计划 + 新建"),
            "拒绝信息应提示改价路径: {err}"
        );
    }

    // 拒绝后计划未被改动
    let (note, amount): (Option<String>, i64) = conn
        .query_row(
            "SELECT note,amount_cents FROM scheduled_transactions WHERE id=?1",
            params![plan_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(note.as_deref(), Some("视频会员"), "备注不应被改动");
    assert_eq!(amount, 3000, "金额不应被改动");
}
