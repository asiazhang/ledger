//! 订阅编辑——仅非金额字段（issue #162，ADR-0023 决策三）：备注 / 分类 / 账户 /
//! 商户编辑与金额字段拒绝，计划生成交易的备注 / 分类 / 账户落库断言。

use cucumber::{then, when};
use rusqlite::params;

use tauri_app_lib::scheduled_transactions::{UpdateSubscriptionInput, update_subscription};

use crate::common::assert_last_error_contains;
use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// 订阅编辑——仅非金额字段（issue #162，ADR-0023 决策三）
// ---------------------------------------------------------------------------

/// 按名称解析支出分类 id（与 budget_steps 的夹具同一张表）。
fn category_id_by_name(conn: &rusqlite::Connection, name: &str) -> Option<String> {
    conn.query_row(
        "SELECT id FROM categories WHERE name=?1 AND kind='expense' AND is_deleted=0",
        params![name],
        |r| r.get::<_, String>(0),
    )
    .ok()
}

/// 编辑最近订阅计划的备注/分类（走 update_subscription 命令体，账户不变）。
#[when(expr = "编辑该订阅计划 备注 {string} 分类 {string}")]
fn edit_subscription_plan(world: &mut LedgerWorld, note: String, category: String) {
    edit_subscription_plan_inner(world, note, Some(category), None);
}

/// 编辑最近订阅计划的商户（issue #190：改商户只影响未来期次；其余字段取当前值）。
#[when(expr = "编辑该订阅计划 商户 {string}")]
fn edit_subscription_plan_merchant(world: &mut LedgerWorld, merchant: String) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let (account_id, category_id, note): (String, Option<String>, Option<String>) =
        world_conn!(world)
            .query_row(
                "SELECT account_id,category_id,note FROM scheduled_transactions WHERE id=?1",
                params![plan_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
    world
        .db
        .write(|conn| {
            update_subscription(
                conn,
                UpdateSubscriptionInput {
                    id: plan_id,
                    account_id,
                    category_id,
                    note,
                    merchant_id: Some(world.merchant_id(&merchant)),
                    amount_cents: false,
                    total_amount_cents: false,
                },
            )
        })
        .expect("编辑订阅商户失败");
}

/// 编辑最近订阅计划的备注/分类/扣款账户（改户只影响未来期次，issue #162）。
#[when(expr = "编辑该订阅计划 备注 {string} 分类 {string} 账户 {string}")]
fn edit_subscription_plan_with_account(
    world: &mut LedgerWorld,
    note: String,
    category: String,
    account: String,
) {
    edit_subscription_plan_inner(world, note, Some(category), Some(account));
}

fn edit_subscription_plan_inner(
    world: &mut LedgerWorld,
    note: String,
    category: Option<String>,
    account: Option<String>,
) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let (current_account_id, current_category_id): (String, Option<String>) = world_conn!(world)
        .query_row(
            "SELECT account_id,category_id FROM scheduled_transactions WHERE id=?1",
            params![plan_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    // 商户为全量替换语义：合法编辑补齐当前商户（含软删商户保持历史引用）。
    let current_merchant: Option<String> = world_conn!(world)
        .query_row(
            "SELECT merchant_id FROM subscription_plans WHERE scheduled_transaction_id=?1",
            params![plan_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .unwrap();
    let category_id = match &category {
        Some(name) => Some(
            category_id_by_name(&world_conn!(world), name)
                .unwrap_or_else(|| panic!("支出分类 '{name}' 不存在")),
        ),
        None => current_category_id,
    };
    world
        .db
        .write(|conn| {
            update_subscription(
                conn,
                UpdateSubscriptionInput {
                    id: plan_id,
                    account_id: account
                        .map(|name| world.account_id(&name))
                        .unwrap_or(current_account_id),
                    category_id,
                    note: Some(note),
                    merchant_id: current_merchant,
                    // 合法编辑请求不携带金额字段
                    amount_cents: false,
                    total_amount_cents: false,
                },
            )
        })
        .expect("编辑订阅计划失败");
}

/// 携带金额字段发出编辑请求：应被后端显式拒绝（ADR-0023 决策三）。
#[when(expr = "携带金额 {int} 编辑该订阅计划")]
fn edit_subscription_plan_with_amount(world: &mut LedgerWorld, _amount: i64) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let (account_id, category_id, note): (String, Option<String>, Option<String>) =
        world_conn!(world)
            .query_row(
                "SELECT account_id,category_id,note FROM scheduled_transactions WHERE id=?1",
                params![plan_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
    let merchant_id: Option<String> = world_conn!(world)
        .query_row(
            "SELECT merchant_id FROM subscription_plans WHERE scheduled_transaction_id=?1",
            params![plan_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .unwrap();
    match world.db.write(|conn| {
        update_subscription(
            conn,
            UpdateSubscriptionInput {
                id: plan_id,
                account_id,
                category_id,
                note,
                merchant_id,
                // 金额哨兵置位：请求携带 amount_cents，应被领域层显式拒绝
                amount_cents: true,
                total_amount_cents: false,
            },
        )
    }) {
        Ok(_) => world.last_error = None,
        Err(e) => world.last_error = Some(e.to_string()),
    }
}

#[then(expr = "编辑应失败并提示 {string}")]
fn assert_edit_error(world: &mut LedgerWorld, needle: String) {
    assert_last_error_contains(world, &needle);
}

#[then(expr = "该计划备注应为 {string}")]
fn assert_plan_note(world: &mut LedgerWorld, expected: String) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let note: String = world_conn!(world)
        .query_row(
            "SELECT note FROM scheduled_transactions WHERE id=?1",
            params![plan_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(note, expected, "计划备注不符");
}

/// 最近计划生成的第 n 笔交易（1 起，按交易日期升序）的落库备注/分类。
#[then(expr = "第 {int} 笔计划交易备注应为 {string}")]
fn assert_plan_txn_note(world: &mut LedgerWorld, nth: usize, expected: String) {
    let txn = plan_generated_txn(world, nth);
    assert_eq!(
        txn.note.as_deref(),
        Some(expected.as_str()),
        "第 {nth} 笔计划交易备注不符"
    );
}

#[then(expr = "第 {int} 笔计划交易备注应为 {string} 分类应为 {string}")]
fn assert_plan_txn_note_and_category(
    world: &mut LedgerWorld,
    nth: usize,
    expected_note: String,
    expected_category: String,
) {
    let txn = plan_generated_txn(world, nth);
    assert_eq!(
        txn.note.as_deref(),
        Some(expected_note.as_str()),
        "第 {nth} 笔计划交易备注不符"
    );
    let category_id = category_id_by_name(&world_conn!(world), &expected_category)
        .unwrap_or_else(|| panic!("支出分类 '{expected_category}' 不存在"));
    assert_eq!(
        txn.category_id.as_deref(),
        Some(category_id.as_str()),
        "第 {nth} 笔计划交易分类不符"
    );
}

struct PlanTxnRow {
    note: Option<String>,
    category_id: Option<String>,
    account_id: String,
}

fn plan_generated_txn(world: &LedgerWorld, nth: usize) -> PlanTxnRow {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    world_conn!(world)
        .query_row(
            "SELECT t.note,t.category_id,t.account_id FROM transactions t \
             JOIN scheduled_transaction_occurrences o ON o.transaction_id=t.id \
             WHERE o.scheduled_transaction_id=?1 AND o.is_deleted=0 \
             ORDER BY t.date ASC, t.created_at ASC LIMIT 1 OFFSET ?2",
            params![plan_id, (nth - 1) as i64],
            |r| {
                Ok(PlanTxnRow {
                    note: r.get(0)?,
                    category_id: r.get(1)?,
                    account_id: r.get(2)?,
                })
            },
        )
        .unwrap_or_else(|e| panic!("计划应已生成第 {nth} 笔交易: {e}"))
}

#[then(expr = "第 {int} 笔计划交易账户应为 {string}")]
fn assert_plan_txn_account(world: &mut LedgerWorld, nth: usize, expected: String) {
    let txn = plan_generated_txn(world, nth);
    assert_eq!(
        txn.account_id,
        world.account_id(&expected),
        "第 {nth} 笔计划交易扣款账户不符"
    );
}

#[then(expr = "该计划扣款账户应为 {string}")]
fn assert_plan_account(world: &mut LedgerWorld, expected: String) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let account_id: String = world_conn!(world)
        .query_row(
            "SELECT account_id FROM scheduled_transactions WHERE id=?1",
            params![plan_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(account_id, world.account_id(&expected), "计划扣款账户不符");
}
