//! 执行与币种折算（issue #59 / spec #52）：`execute_occurrence` 落库路径已收敛到
//! Writer 接缝（`writer::normalize` + `insert_row`）——非默认币种折算回归、
//! expense / transfer 映射、计划/期次状态流转锁定。

use super::super::*;
use super::common::{
    create_installment, create_subscription, create_transfer_plan, first_pending_occurrence,
    insert_account, insert_rate, occurrence_status, read_txn, setup_db,
};
use rusqlite::{Connection, params};

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
// 引擎事务自持：期次中间态缺口修复（issue #230 / ADR-0033 决策 #6）
// ---------------------------------------------------------------------------

/// 注入「期次落库中途失败」：期次已 CAS 置 processing 后，交易行 INSERT 被触发器
/// RAISE(ABORT) 挡下——纯测试侧手段（spec #169 定案），产品代码零 hook。
fn inject_txn_insert_failure(conn: &Connection) {
    conn.execute(
        "CREATE TRIGGER block_txn_insert BEFORE INSERT ON transactions \
         BEGIN SELECT RAISE(ABORT, '测试注入：期次落库失败'); END",
        [],
    )
    .unwrap();
}

/// 期次落库中途失败 → 无交易残留、期次回原状态 pending 可重试（不滞留
/// processing），移除注入后同一期次重试成功。
#[test]
fn execute_occurrence_mid_failure_rolls_back_to_pending() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let plan_id = create_subscription(&conn, "acc", "CNY", 3000, Some("回滚订阅"));
    let occ_id = first_pending_occurrence(&conn, &plan_id);
    inject_txn_insert_failure(&conn);

    let err = execute_occurrence(&conn, &occ_id).unwrap_err();
    assert!(
        err.to_string().contains("测试注入"),
        "应上抛注入的落库错误，实际: {err:?}"
    );

    // 数据终态：交易行无残留、期次回原状态、未回填交易 id。
    let txn_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(txn_count, 0, "中途失败不应残留交易行");
    let (status, backfilled) = occurrence_status(&conn, &occ_id);
    assert_eq!(status, "pending", "期次应回原状态可重试，不滞留 processing");
    assert_eq!(backfilled, None, "失败不应回填交易 id");

    // 移除注入后同一期次可重试成功。
    conn.execute("DROP TRIGGER block_txn_insert", []).unwrap();
    let txn_id = execute_occurrence(&conn, &occ_id).unwrap();
    let (status, backfilled) = occurrence_status(&conn, &occ_id);
    assert_eq!(status, "completed");
    assert_eq!(backfilled.as_deref(), Some(txn_id.as_str()));
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
            merchant_id: None,
            policy_id: None,
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
    assert_eq!(err.to_string(), "关联计划未处于活跃状态");
}
