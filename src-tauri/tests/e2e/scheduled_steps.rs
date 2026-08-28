//! 定时交易引擎（issue #71）的 BDD 步骤：在命令 seam 上断言
//! - 引擎生成的交易经 transaction::writer 落库，`amount_native_cents` 由
//!   convert_to_native 折算（非硬编码 1:1），缺汇率报错且期次保持可重试；
//! - 分期 / 订阅 / 定时转账生成的类型与金额不回归。
//!
//! 步骤直接调 `scheduled_transactions` 领域函数（即 `commands::scheduled` 的
//! 命令体），与 transactions_steps 直调命令函数的 seam 一致。

use cucumber::{given, then, when};
use rusqlite::params;

use tauri_app_lib::scheduled_transactions::{
    CreateScheduledInput, RecurrenceType, ScheduledKind, create_plan, execute_occurrence,
};

use crate::common::assert_last_error_contains;
use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

/// 写入一条汇率（base → quote）。币种须存在于种子 currencies（FK 约束）。
#[given(expr = "存在汇率 {string} 兑 {string} 为 {float}")]
fn add_exchange_rate(world: &mut LedgerWorld, base: String, quote: String, rate: f64) {
    world
        .conn
        .execute(
            "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,updated_at,version,device_id) \
             VALUES ('er-' || hex(randomblob(8)), ?1, ?2, ?3, '2026-02-01T00:00:00Z','2026-02-01T00:00:00Z',1,'test')",
            params![base, quote, rate],
        )
        .unwrap();
}

// ---------------------------------------------------------------------------
// When：创建计划
// ---------------------------------------------------------------------------

#[when(expr = "创建订阅计划 金额 {int} 币种 {string} 账户 {string} 起始日期 {string}")]
fn create_subscription_plan(
    world: &mut LedgerWorld,
    amount: i64,
    currency: String,
    account: String,
    start: String,
) {
    create_subscription_plan_inner(world, amount, currency, account, start, None);
}

#[when(
    expr = "创建订阅计划 金额 {int} 币种 {string} 账户 {string} 起始日期 {string} 备注 {string}"
)]
fn create_subscription_plan_with_note(
    world: &mut LedgerWorld,
    amount: i64,
    currency: String,
    account: String,
    start: String,
    note: String,
) {
    create_subscription_plan_inner(world, amount, currency, account, start, Some(note));
}

fn create_subscription_plan_inner(
    world: &mut LedgerWorld,
    amount: i64,
    currency: String,
    account: String,
    start: String,
    note: Option<String>,
) {
    let id = create_plan(
        &world.conn,
        CreateScheduledInput {
            kind: ScheduledKind::Subscription,
            account_id: world.account_id(&account),
            category_id: None,
            amount_cents: amount,
            currency_code: currency,
            recurrence_type: RecurrenceType::Monthly,
            recurrence_interval: 1,
            recurrence_day: None,
            start_date: start,
            note,
            counterparty: Some("订阅".into()),
            total_amount_cents: None,
            total_occurrences: None,
            to_account_id: None,
        },
    )
    .expect("创建订阅计划失败");
    world.last_plan_id = Some(id);
}

#[when(expr = "创建分期计划 总额 {int} 期数 {int} 账户 {string} 起始日期 {string}")]
fn create_installment_plan(
    world: &mut LedgerWorld,
    total: i64,
    occurrences: i64,
    account: String,
    start: String,
) {
    let id = create_plan(
        &world.conn,
        CreateScheduledInput {
            kind: ScheduledKind::Installment,
            account_id: world.account_id(&account),
            category_id: None,
            amount_cents: total / occurrences,
            currency_code: "CNY".into(),
            recurrence_type: RecurrenceType::Monthly,
            recurrence_interval: 1,
            recurrence_day: None,
            start_date: start,
            note: None,
            counterparty: Some("分期".into()),
            total_amount_cents: Some(total),
            total_occurrences: Some(occurrences),
            to_account_id: None,
        },
    )
    .expect("创建分期计划失败");
    world.last_plan_id = Some(id);
}

#[when(expr = "创建定时转账计划 金额 {int} 从 {string} 到 {string} 期数 {int} 起始日期 {string}")]
fn create_scheduled_transfer_plan(
    world: &mut LedgerWorld,
    amount: i64,
    from: String,
    to: String,
    occurrences: i64,
    start: String,
) {
    let id = create_plan(
        &world.conn,
        CreateScheduledInput {
            kind: ScheduledKind::ScheduledTransfer,
            account_id: world.account_id(&from),
            category_id: None,
            amount_cents: amount,
            currency_code: "CNY".into(),
            recurrence_type: RecurrenceType::Monthly,
            recurrence_interval: 1,
            recurrence_day: None,
            start_date: start,
            note: None,
            counterparty: None,
            total_amount_cents: None,
            total_occurrences: Some(occurrences),
            to_account_id: Some(world.account_id(&to)),
        },
    )
    .expect("创建定时转账计划失败");
    world.last_plan_id = Some(id);
}

// ---------------------------------------------------------------------------
// When：执行期次
// ---------------------------------------------------------------------------

/// 执行最近计划的第一个 pending 期次（按 scheduled_date 升序）。
#[when(expr = "执行该计划第一期")]
fn execute_first_occurrence(world: &mut LedgerWorld) {
    let occ_id = pending_occurrence_ids(world, Some(1))
        .into_iter()
        .next()
        .expect("计划应已有 pending 期次");
    execute_occurrence_step(world, &occ_id);
}

/// 依次执行最近计划的全部 pending 期次（按 scheduled_date 升序）。
#[when(expr = "依次执行全部期次")]
fn execute_all_occurrences(world: &mut LedgerWorld) {
    for occ_id in pending_occurrence_ids(world, None) {
        execute_occurrence_step(world, &occ_id);
    }
}

/// 最近计划的 pending 期次 id（按 scheduled_date 升序；`limit` 为 None 时取全部）。
fn pending_occurrence_ids(world: &LedgerWorld, limit: Option<i64>) -> Vec<String> {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let mut stmt = world
        .conn
        .prepare(
            "SELECT id FROM scheduled_transaction_occurrences \
             WHERE scheduled_transaction_id=?1 AND status='pending' AND is_deleted=0 \
             ORDER BY scheduled_date ASC LIMIT ?2",
        )
        .unwrap();
    stmt.query_map(params![plan_id, limit.unwrap_or(i64::MAX)], |r| {
        r.get::<_, String>(0)
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

/// 重新执行上一次尝试的期次（失败场景中补录汇率后重试）。
#[when(expr = "重新执行该期次")]
fn re_execute_occurrence(world: &mut LedgerWorld) {
    let occ_id = world.last_occurrence_id.clone().expect("尚无期次");
    execute_occurrence_step(world, &occ_id);
}

/// 执行期次并记录结果：成功回填 last_transaction_id，失败记录 last_error。
fn execute_occurrence_step(world: &mut LedgerWorld, occ_id: &str) {
    world.last_occurrence_id = Some(occ_id.to_string());
    match execute_occurrence(&world.conn, occ_id) {
        Ok(txn_id) => {
            world.last_transaction_id = Some(txn_id);
            world.last_error = None;
        }
        Err(e) => {
            world.last_transaction_id = None;
            world.last_error = Some(e.to_string());
        }
    }
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "执行应失败并提示 {string}")]
fn assert_last_error(world: &mut LedgerWorld, needle: String) {
    assert_last_error_contains(world, &needle);
}

#[then(expr = "期次未回填交易")]
fn assert_occurrence_not_backfilled(world: &mut LedgerWorld) {
    let occ_id = world.last_occurrence_id.clone().expect("尚无期次");
    let txn_id: Option<String> = world
        .conn
        .query_row(
            "SELECT transaction_id FROM scheduled_transaction_occurrences WHERE id=?1",
            params![occ_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(txn_id.is_none(), "失败期次不应回填交易 id");
}

#[then(expr = "该期次状态应为 {string}")]
fn assert_occurrence_status(world: &mut LedgerWorld, expected: String) {
    let occ_id = world.last_occurrence_id.clone().expect("尚无期次");
    let status: String = world
        .conn
        .query_row(
            "SELECT status FROM scheduled_transaction_occurrences WHERE id=?1",
            params![occ_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, expected, "期次状态不符");
}

#[then(expr = "该期次交易类型应为 {string} 金额应为 {int}")]
fn assert_occurrence_txn_kind_amount(
    world: &mut LedgerWorld,
    expected_kind: String,
    expected_amount: i64,
) {
    let txn = occurrence_txn(world);
    assert_eq!(txn.kind, expected_kind, "交易类型不符");
    assert_eq!(txn.amount_cents, expected_amount, "原始币种金额不符");
}

#[then(expr = "该期次交易本位币金额应为 {int}")]
fn assert_occurrence_txn_native(world: &mut LedgerWorld, expected: i64) {
    let txn = occurrence_txn(world);
    assert_eq!(
        txn.amount_native_cents, expected,
        "本位币金额应经 convert_to_native 折算"
    );
}

#[then(expr = "该期次交易转入账户应为 {string}")]
fn assert_occurrence_txn_to_account(world: &mut LedgerWorld, account_name: String) {
    let txn = occurrence_txn(world);
    let to_account_id = txn.to_account_id.expect("转账交易应有转入账户");
    assert_eq!(
        to_account_id,
        world.account_id(&account_name),
        "转入账户不符"
    );
}

#[then(expr = "应生成 {int} 笔类型 {string} 的交易 金额依次为 {string}")]
fn assert_generated_txns(world: &mut LedgerWorld, count: i64, kind: String, amounts_csv: String) {
    let expected: Vec<i64> = amounts_csv
        .split(',')
        .map(|s| s.trim().parse().expect("金额列表应为逗号分隔整数"))
        .collect();
    assert_eq!(expected.len() as i64, count, "金额列表长度应与笔数一致");
    // 只统计最近计划的期次回填的交易（经 occurrence 关联），避免整库 kind 误计。
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let amounts: Vec<i64> = {
        let mut stmt = world
            .conn
            .prepare(
                "SELECT t.amount_cents FROM transactions t \
                 JOIN scheduled_transaction_occurrences o ON o.transaction_id=t.id \
                 WHERE o.scheduled_transaction_id=?1 AND t.kind=?2 AND t.is_deleted=0 \
                 ORDER BY t.date ASC, t.created_at ASC",
            )
            .unwrap();
        stmt.query_map(params![plan_id, kind], |r| r.get::<_, i64>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    };
    assert_eq!(amounts.len() as i64, count, "计划生成的交易笔数不符");
    assert_eq!(amounts, expected, "各期金额应与分期计划一致");
}

#[then(expr = "计划状态应为 {string}")]
fn assert_plan_status(world: &mut LedgerWorld, expected: String) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let status: String = world
        .conn
        .query_row(
            "SELECT status FROM scheduled_transactions WHERE id=?1",
            params![plan_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, expected, "计划状态不符");
}

/// 最近期次回填的交易的落库字段（供断言与 writer 列映射一致）。
struct OccurrenceTxn {
    kind: String,
    amount_cents: i64,
    amount_native_cents: i64,
    to_account_id: Option<String>,
}

fn occurrence_txn(world: &LedgerWorld) -> OccurrenceTxn {
    let occ_id = world.last_occurrence_id.clone().expect("尚无期次");
    let txn_id: Option<String> = world
        .conn
        .query_row(
            "SELECT transaction_id FROM scheduled_transaction_occurrences WHERE id=?1",
            params![occ_id],
            |r| r.get(0),
        )
        .unwrap();
    let txn_id = txn_id.expect("期次尚未回填交易 id");
    world
        .conn
        .query_row(
            "SELECT kind,amount_cents,amount_native_cents,to_account_id FROM transactions WHERE id=?1",
            params![txn_id],
            |r| {
                Ok(OccurrenceTxn {
                    kind: r.get(0)?,
                    amount_cents: r.get(1)?,
                    amount_native_cents: r.get(2)?,
                    to_account_id: r.get(3)?,
                })
            },
        )
        .unwrap()
}

// ---------------------------------------------------------------------------
// 订阅花费——实际花费口径（issue #160，ADR-0023 决策二）
// ---------------------------------------------------------------------------

use tauri_app_lib::scheduled_transactions::{
    ScheduledStatus, SubscriptionSpendOverview, UpdateSubscriptionInput, query_subscription_spend,
    update_plan_status, update_subscription,
};

#[when(
    expr = "创建订阅计划 金额 {int} 币种 {string} 账户 {string} 周期 {string} 起始日期 {string} 备注 {string}"
)]
fn create_subscription_plan_with_recurrence(
    world: &mut LedgerWorld,
    amount: i64,
    currency: String,
    account: String,
    recurrence: String,
    start: String,
    note: String,
) {
    let recurrence_type: RecurrenceType = recurrence
        .parse()
        .expect("周期应为 daily/weekly/monthly/yearly");
    let id = create_plan(
        &world.conn,
        CreateScheduledInput {
            kind: ScheduledKind::Subscription,
            account_id: world.account_id(&account),
            category_id: None,
            amount_cents: amount,
            currency_code: currency,
            recurrence_type,
            recurrence_interval: 1,
            recurrence_day: None,
            start_date: start,
            note: Some(note),
            counterparty: Some("订阅".into()),
            total_amount_cents: None,
            total_occurrences: None,
            to_account_id: None,
        },
    )
    .expect("创建订阅计划失败");
    world.last_plan_id = Some(id);
}

/// 执行最近计划的前 N 条 pending 期次（scheduled_date 升序）。
#[when(expr = "执行该计划前 {int} 期")]
fn execute_first_n_occurrences(world: &mut LedgerWorld, n: usize) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let occ_ids: Vec<String> = {
        let mut stmt = world
            .conn
            .prepare(
                "SELECT id FROM scheduled_transaction_occurrences \
                 WHERE scheduled_transaction_id=?1 AND status='pending' AND is_deleted=0 \
                 ORDER BY scheduled_date ASC LIMIT ?2",
            )
            .unwrap();
        stmt.query_map(params![plan_id, n as i64], |r| r.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    };
    for occ_id in occ_ids {
        execute_occurrence_step(world, &occ_id);
    }
}

/// 取消最近的订阅计划（走 update_plan_status 命令体）。
#[when(expr = "取消该订阅计划")]
fn cancel_subscription_plan(world: &mut LedgerWorld) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    update_plan_status(&world.conn, &plan_id, ScheduledStatus::Cancelled)
        .expect("取消订阅计划失败");
}

/// 暂停最近的订阅计划（走 update_plan_status 命令体）。
#[when(expr = "暂停该订阅计划")]
fn pause_subscription_plan(world: &mut LedgerWorld) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    update_plan_status(&world.conn, &plan_id, ScheduledStatus::Paused).expect("暂停订阅计划失败");
}

/// 以注入的固定「今日」查询订阅实际花费总览（确定性口径，不依赖真实时钟）。
#[when(expr = "以 {string} 为今日查询订阅花费")]
fn query_spend_with_today(world: &mut LedgerWorld, today: String) {
    let today =
        chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d").expect("今日日期应为 YYYY-MM-DD");
    world.last_spend =
        Some(query_subscription_spend(&world.conn, today).expect("查询订阅花费失败"));
}

fn last_spend(world: &LedgerWorld) -> &SubscriptionSpendOverview {
    world.last_spend.as_ref().expect("尚未查询订阅花费")
}

#[then(expr = "本月实际花费应为 {int}")]
fn assert_spend_this_month(world: &mut LedgerWorld, expected: i64) {
    assert_eq!(last_spend(world).this_month_native_cents, expected);
}

#[then(expr = "本年实际花费应为 {int}")]
fn assert_spend_this_year(world: &mut LedgerWorld, expected: i64) {
    assert_eq!(last_spend(world).this_year_native_cents, expected);
}

#[then(expr = "折算月成本应为 {int}")]
fn assert_projected_month(world: &mut LedgerWorld, expected: i64) {
    assert_eq!(last_spend(world).projected_month_native_cents, expected);
}

#[then(expr = "折算年成本应为 {int}")]
fn assert_projected_year(world: &mut LedgerWorld, expected: i64) {
    assert_eq!(last_spend(world).projected_year_native_cents, expected);
}

#[then(expr = "近 12 个月中 {string} 实际花费应为 {int}")]
fn assert_spend_month(world: &mut LedgerWorld, month: String, expected: i64) {
    let overview = last_spend(world);
    let cents = overview
        .months
        .iter()
        .find(|m| m.month == month)
        .unwrap_or_else(|| panic!("12 个月序列应包含 {month}"))
        .native_cents;
    assert_eq!(cents, expected, "{month} 实际花费不符");
}

#[then(expr = "订阅花费行数应为 {int}")]
fn assert_spend_row_count(world: &mut LedgerWorld, expected: usize) {
    assert_eq!(last_spend(world).rows.len(), expected);
}

#[then(expr = "订阅行 {string} 状态应为 {string}")]
fn assert_spend_row_status(world: &mut LedgerWorld, note: String, status: String) {
    let row = last_spend(world)
        .rows
        .iter()
        .find(|r| r.note.as_deref() == Some(note.as_str()))
        .unwrap_or_else(|| panic!("订阅花费行应包含备注 {note}"));
    assert_eq!(row.status, status);
}

#[then(expr = "订阅行 {string} 本年实际花费应为 {int}")]
fn assert_spend_row_year(world: &mut LedgerWorld, note: String, expected: i64) {
    let row = last_spend(world)
        .rows
        .iter()
        .find(|r| r.note.as_deref() == Some(note.as_str()))
        .unwrap_or_else(|| panic!("订阅花费行应包含备注 {note}"));
    assert_eq!(row.this_year_native_cents, expected);
}

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

/// 编辑最近订阅计划的备注与分类（走 update_subscription 命令体，账户不变）。
#[when(expr = "编辑该订阅计划 备注 {string} 分类 {string}")]
fn edit_subscription_plan(world: &mut LedgerWorld, note: String, category: String) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let account_id: String = world
        .conn
        .query_row(
            "SELECT account_id FROM scheduled_transactions WHERE id=?1",
            params![plan_id],
            |r| r.get(0),
        )
        .unwrap();
    let category_id = category_id_by_name(&world.conn, &category)
        .unwrap_or_else(|| panic!("支出分类 '{category}' 不存在"));
    update_subscription(
        &world.conn,
        UpdateSubscriptionInput {
            id: plan_id,
            account_id,
            category_id: Some(category_id),
            note: Some(note),
            // 合法编辑请求不携带金额字段
            amount_cents: None,
            total_amount_cents: None,
        },
    )
    .expect("编辑订阅计划失败");
}

/// 携带金额字段发出编辑请求：应被后端显式拒绝（ADR-0023 决策三）。
#[when(expr = "携带金额 {int} 编辑该订阅计划")]
fn edit_subscription_plan_with_amount(world: &mut LedgerWorld, amount: i64) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let (account_id, category_id, note): (String, Option<String>, Option<String>) = world
        .conn
        .query_row(
            "SELECT account_id,category_id,note FROM scheduled_transactions WHERE id=?1",
            params![plan_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    match update_subscription(
        &world.conn,
        UpdateSubscriptionInput {
            id: plan_id,
            account_id,
            category_id,
            note,
            amount_cents: Some(amount),
            total_amount_cents: None,
        },
    ) {
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
    let note: String = world
        .conn
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
    let category_id = category_id_by_name(&world.conn, &expected_category)
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
}

fn plan_generated_txn(world: &LedgerWorld, nth: usize) -> PlanTxnRow {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    world
        .conn
        .query_row(
            "SELECT t.note,t.category_id FROM transactions t \
             JOIN scheduled_transaction_occurrences o ON o.transaction_id=t.id \
             WHERE o.scheduled_transaction_id=?1 AND o.is_deleted=0 \
             ORDER BY t.date ASC, t.created_at ASC LIMIT 1 OFFSET ?2",
            params![plan_id, (nth - 1) as i64],
            |r| {
                Ok(PlanTxnRow {
                    note: r.get(0)?,
                    category_id: r.get(1)?,
                })
            },
        )
        .unwrap_or_else(|e| panic!("计划应已生成第 {nth} 笔交易: {e}"))
}
