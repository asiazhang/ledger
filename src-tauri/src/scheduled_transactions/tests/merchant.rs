//! 商户复制与软删引用（issue #190 / ADR-0028）：计划带商户 → 每期生成交易复制商户到流水；
//! 软删商户不可被新计划选择，历史引用照常保持，编辑商户仅影响未来期次。

use super::super::*;
use super::common::{first_pending_occurrence, insert_account, read_txn, setup_db};
use rusqlite::Connection;
use rusqlite::params;

// ---------------------------------------------------------------------------
// 商户复制（issue #190 / ADR-0028）：计划带商户 → 每期生成交易复制商户到流水
// ---------------------------------------------------------------------------

/// 插入一个在用商户，返回其 id。
fn insert_merchant(conn: &Connection, name: &str) -> String {
    let id = format!("mer-{name}");
    conn.execute(
        "INSERT INTO merchants (id,name,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        params![id, name],
    )
    .unwrap();
    id
}

/// 软删商户（走 delete_merchant_internal 命令体）。
fn soft_delete_merchant(conn: &Connection, id: &str) {
    crate::commands::merchants::delete_merchant_internal(conn, id).unwrap();
}

/// 创建带商户的订阅计划，返回计划 id。
fn create_subscription_with_merchant(
    conn: &Connection,
    account_id: &str,
    merchant_id: &str,
) -> String {
    create_plan(
        conn,
        CreateScheduledInput {
            kind: ScheduledKind::Subscription,
            account_id: account_id.into(),
            category_id: None,
            amount_cents: 3000,
            currency_code: "CNY".into(),
            recurrence_type: RecurrenceType::Monthly,
            recurrence_interval: 1,
            recurrence_day: None,
            start_date: "2026-01-15".into(),
            note: None,
            merchant_id: Some(merchant_id.into()),
            policy_id: None,
            total_amount_cents: None,
            total_occurrences: None,
            to_account_id: None,
        },
    )
    .unwrap()
}

/// 订阅计划带商户：每期生成交易复制计划的商户到流水。
#[test]
fn subscription_copies_merchant_to_generated_transaction() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let merchant_id = insert_merchant(&conn, "Netflix");
    let plan_id = create_subscription_with_merchant(&conn, "acc", &merchant_id);

    let txn_id = execute_occurrence(&conn, &first_pending_occurrence(&conn, &plan_id)).unwrap();
    let txn = read_txn(&conn, &txn_id);
    assert_eq!(
        txn.merchant_id.as_deref(),
        Some(merchant_id.as_str()),
        "流水应复制计划的商户"
    );
}

/// 分期计划带商户：每期生成交易复制计划的商户到流水。
#[test]
fn installment_copies_merchant_to_generated_transaction() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let merchant_id = insert_merchant(&conn, "京东白条");
    let plan_id = create_plan(
        &conn,
        CreateScheduledInput {
            kind: ScheduledKind::Installment,
            account_id: "acc".into(),
            category_id: None,
            amount_cents: 1000,
            currency_code: "CNY".into(),
            recurrence_type: RecurrenceType::Monthly,
            recurrence_interval: 1,
            recurrence_day: None,
            start_date: "2026-01-15".into(),
            note: None,
            merchant_id: Some(merchant_id.clone()),
            policy_id: None,
            total_amount_cents: Some(3000),
            total_occurrences: Some(3),
            to_account_id: None,
        },
    )
    .unwrap();

    let txn_id = execute_occurrence(&conn, &first_pending_occurrence(&conn, &plan_id)).unwrap();
    let txn = read_txn(&conn, &txn_id);
    assert_eq!(txn.merchant_id.as_deref(), Some(merchant_id.as_str()));
}

/// 定时转账行为层拒绝携带商户（用 to_account_id 表示本方账户间转账，ADR-0028）。
#[test]
fn scheduled_transfer_rejects_merchant() {
    let conn = setup_db();
    insert_account(&conn, "acc-a", "CNY");
    insert_account(&conn, "acc-b", "CNY");
    let merchant_id = insert_merchant(&conn, "京东");

    let err = create_plan(
        &conn,
        CreateScheduledInput {
            kind: ScheduledKind::ScheduledTransfer,
            account_id: "acc-a".into(),
            category_id: None,
            amount_cents: 50000,
            currency_code: "CNY".into(),
            recurrence_type: RecurrenceType::Monthly,
            recurrence_interval: 1,
            recurrence_day: None,
            start_date: "2026-01-15".into(),
            note: None,
            merchant_id: Some(merchant_id),
            policy_id: None,
            total_amount_cents: None,
            total_occurrences: Some(3),
            to_account_id: Some("acc-b".into()),
        },
    )
    .expect_err("定时转账携带商户应被行为层拒绝");
    assert!(
        err.to_string().contains("定时转账不能携带商户"),
        "实际: {err}"
    );
}

/// 创建计划携带已软删商户 → 拒绝（软删商户不可再被新计划选择）。
#[test]
fn create_plan_rejects_soft_deleted_merchant() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let merchant_id = insert_merchant(&conn, "京东");
    soft_delete_merchant(&conn, &merchant_id);

    let err = create_plan(
        &conn,
        CreateScheduledInput {
            kind: ScheduledKind::Subscription,
            account_id: "acc".into(),
            category_id: None,
            amount_cents: 3000,
            currency_code: "CNY".into(),
            recurrence_type: RecurrenceType::Monthly,
            recurrence_interval: 1,
            recurrence_day: None,
            start_date: "2026-01-15".into(),
            note: None,
            merchant_id: Some(merchant_id),
            policy_id: None,
            total_amount_cents: None,
            total_occurrences: None,
            to_account_id: None,
        },
    )
    .expect_err("软删商户不应可被新计划选择");
    assert!(
        err.to_string().contains("商户不存在或已删除"),
        "实际: {err}"
    );
}

/// 计划商户被软删后，期次仍复制该历史引用（照常执行，不因校验失败卡住）。
#[test]
fn occurrence_keeps_plan_merchant_after_soft_delete() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let merchant_id = insert_merchant(&conn, "京东");
    let plan_id = create_subscription_with_merchant(&conn, "acc", &merchant_id);

    // 第一期正常执行（商户在用）
    let txn1 = execute_occurrence(&conn, &first_pending_occurrence(&conn, &plan_id)).unwrap();
    assert_eq!(
        read_txn(&conn, &txn1).merchant_id.as_deref(),
        Some(merchant_id.as_str())
    );

    // 软删商户后：第二期仍照常执行并复制历史引用（不报「商户不存在或已删除」）
    soft_delete_merchant(&conn, &merchant_id);
    let occ_ids: Vec<String> = get_plan_detail(&conn, &plan_id)
        .unwrap()
        .pending_occurrences
        .into_iter()
        .map(|o| o.id)
        .collect();
    let txn2 = execute_occurrence(&conn, &occ_ids[0]).unwrap();
    assert_eq!(
        read_txn(&conn, &txn2).merchant_id.as_deref(),
        Some(merchant_id.as_str()),
        "软删后历史引用应继续复制"
    );
}

/// 构造订阅编辑入参（其余字段取计划当前值，商户由调用方指定）。
fn update_subscription_input(
    conn: &Connection,
    plan_id: &str,
    merchant_id: Option<String>,
) -> UpdateSubscriptionInput {
    let (account_id, category_id, note): (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT account_id,category_id,note FROM scheduled_transactions WHERE id=?1",
            params![plan_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    UpdateSubscriptionInput {
        id: plan_id.into(),
        account_id,
        category_id,
        note,
        merchant_id,
        amount_cents: false,
        total_amount_cents: false,
    }
}

/// 编辑订阅改商户：只影响未来期次（期次执行时从计划扩展表读商户）。
#[test]
fn update_subscription_merchant_affects_only_future_occurrences() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let mer_a = insert_merchant(&conn, "商户A");
    let mer_b = insert_merchant(&conn, "商户B");
    let plan_id = create_subscription_with_merchant(&conn, "acc", &mer_a);

    // 第一期：商户 A
    let occ_ids: Vec<String> = get_plan_detail(&conn, &plan_id)
        .unwrap()
        .pending_occurrences
        .into_iter()
        .map(|o| o.id)
        .collect();
    let txn1 = execute_occurrence(&conn, &occ_ids[0]).unwrap();
    assert_eq!(
        read_txn(&conn, &txn1).merchant_id.as_deref(),
        Some(mer_a.as_str())
    );

    // 编辑商户 → B：已生成交易不动，未来期次用新商户
    update_subscription(
        &conn,
        update_subscription_input(&conn, &plan_id, Some(mer_b.clone())),
    )
    .unwrap();
    let txn2 = execute_occurrence(&conn, &occ_ids[1]).unwrap();
    assert_eq!(
        read_txn(&conn, &txn2).merchant_id.as_deref(),
        Some(mer_b.as_str()),
        "改商户只影响未来期次"
    );
}

/// 编辑其它字段时商户为全量替换：提交当前值（软删商户）视为保持历史引用，不报错。
#[test]
fn update_subscription_keeps_soft_deleted_merchant_reference() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let merchant_id = insert_merchant(&conn, "京东");
    let plan_id = create_subscription_with_merchant(&conn, "acc", &merchant_id);
    soft_delete_merchant(&conn, &merchant_id);

    // 只改备注（商户字段携带当前软删商户）：保持历史引用，编辑成功
    let mut input = update_subscription_input(&conn, &plan_id, Some(merchant_id.clone()));
    input.note = Some("改备注".into());
    update_subscription(&conn, input).expect("保持软删商户引用不应失败");
    let kept: Option<String> = conn
        .query_row(
            "SELECT merchant_id FROM subscription_plans WHERE scheduled_transaction_id=?1",
            params![plan_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kept.as_deref(), Some(merchant_id.as_str()));
}

/// 编辑改到已软删商户 → 拒绝（软删商户不可被新选择，与创建计划同文案）。
#[test]
fn update_subscription_rejects_soft_deleted_new_merchant() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let mer_a = insert_merchant(&conn, "商户A");
    let mer_b = insert_merchant(&conn, "商户B");
    let plan_id = create_subscription_with_merchant(&conn, "acc", &mer_a);
    soft_delete_merchant(&conn, &mer_b);

    let err = update_subscription(
        &conn,
        update_subscription_input(&conn, &plan_id, Some(mer_b)),
    )
    .expect_err("改到软删商户应被拒绝");
    assert!(
        err.to_string().contains("商户不存在或已删除"),
        "实际: {err}"
    );
}

/// 编辑清空商户（merchant_id → null）：允许（无商户订阅）。
#[test]
fn update_subscription_clears_merchant() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let merchant_id = insert_merchant(&conn, "京东");
    let plan_id = create_subscription_with_merchant(&conn, "acc", &merchant_id);

    update_subscription(&conn, update_subscription_input(&conn, &plan_id, None)).unwrap();
    let cleared: Option<String> = conn
        .query_row(
            "SELECT merchant_id FROM subscription_plans WHERE scheduled_transaction_id=?1",
            params![plan_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cleared, None, "清空商户应置空扩展表 merchant_id");
}
