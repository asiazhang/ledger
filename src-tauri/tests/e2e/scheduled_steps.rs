//! 定时交易引擎（issue #71）的 BDD 步骤：在命令 seam 上断言
//! - 引擎生成的交易经 transaction::writer 落库，`amount_native_cents` 由
//!   convert_to_native 折算（非硬编码 1:1），缺汇率报错且期次保持可重试；
//! - 分期 / 订阅 / 定时转账生成的类型与金额不回归。
//!
//! 步骤直接调 `scheduled_transactions` 领域函数（即 `commands::scheduled` 的
//! 命令体），与 transactions_steps 直调命令函数的 seam 一致。

use cucumber::{given, then, when};
use rusqlite::params;

use tauri_app_lib::error::AppError;
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
            merchant_id: None,
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
            merchant_id: None,
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
            merchant_id: None,
            total_amount_cents: None,
            total_occurrences: Some(occurrences),
            to_account_id: Some(world.account_id(&to)),
        },
    )
    .expect("创建定时转账计划失败");
    world.last_plan_id = Some(id);
}

// ---------------------------------------------------------------------------
// When：带商户的计划（issue #190 / ADR-0028：installment/subscription 可携带商户）
// ---------------------------------------------------------------------------

/// 创建带商户的订阅计划（每期生成交易时复制商户到流水）。
#[when(
    expr = "创建订阅计划 金额 {int} 币种 {string} 账户 {string} 起始日期 {string} 备注 {string} 商户 {string}"
)]
fn create_subscription_plan_with_merchant(
    world: &mut LedgerWorld,
    amount: i64,
    currency: String,
    account: String,
    start: String,
    note: String,
    merchant: String,
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
            note: Some(note),
            merchant_id: Some(world.merchant_id(&merchant)),
            total_amount_cents: None,
            total_occurrences: None,
            to_account_id: None,
        },
    )
    .expect("创建订阅计划失败");
    world.last_plan_id = Some(id);
}

/// 创建带商户的分期计划。
#[when(expr = "创建分期计划 总额 {int} 期数 {int} 账户 {string} 起始日期 {string} 商户 {string}")]
fn create_installment_plan_with_merchant(
    world: &mut LedgerWorld,
    total: i64,
    occurrences: i64,
    account: String,
    start: String,
    merchant: String,
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
            merchant_id: Some(world.merchant_id(&merchant)),
            total_amount_cents: Some(total),
            total_occurrences: Some(occurrences),
            to_account_id: None,
        },
    )
    .expect("创建分期计划失败");
    world.last_plan_id = Some(id);
}

/// 尝试创建定时转账计划并捕获错误（行为层拒绝携带商户，issue #190）。
#[when(
    expr = "尝试创建定时转账计划 金额 {int} 从 {string} 到 {string} 期数 {int} 起始日期 {string} 商户 {string}"
)]
fn try_create_transfer_plan_with_merchant(
    world: &mut LedgerWorld,
    amount: i64,
    from: String,
    to: String,
    occurrences: i64,
    start: String,
    merchant: String,
) {
    let result = create_plan(
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
            merchant_id: Some(world.merchant_id(&merchant)),
            total_amount_cents: None,
            total_occurrences: Some(occurrences),
            to_account_id: Some(world.account_id(&to)),
        },
    );
    world.last_error = match result {
        Err(AppError::Invalid(msg)) => Some(msg),
        _ => Some("预期失败但成功了".into()),
    };
}

/// 尝试创建带商户的订阅计划并捕获错误（软删商户不可被新计划选择）。
#[when(
    expr = "尝试创建订阅计划 金额 {int} 币种 {string} 账户 {string} 起始日期 {string} 商户 {string}"
)]
fn try_create_subscription_plan_with_merchant(
    world: &mut LedgerWorld,
    amount: i64,
    currency: String,
    account: String,
    start: String,
    merchant: String,
) {
    let result = create_plan(
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
            note: None,
            merchant_id: Some(world.merchant_id(&merchant)),
            total_amount_cents: None,
            total_occurrences: None,
            to_account_id: None,
        },
    );
    world.last_error = match result {
        Err(AppError::Invalid(msg)) => Some(msg),
        _ => Some("预期失败但成功了".into()),
    };
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

/// 最近期次生成的交易商户名（左联 merchants 现名：改名即时生效，软删照常显示）。
fn occurrence_txn_merchant_name(world: &LedgerWorld) -> Option<String> {
    let occ_id = world.last_occurrence_id.clone().expect("尚无期次");
    world
        .conn
        .query_row(
            "SELECT m.name FROM scheduled_transaction_occurrences o \
             JOIN transactions t ON t.id = o.transaction_id \
             LEFT JOIN merchants m ON m.id = t.merchant_id \
             WHERE o.id=?1",
            params![occ_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .unwrap()
}

#[then(expr = "该期次交易商户应为 {string}")]
fn assert_occurrence_txn_merchant(world: &mut LedgerWorld, expected: String) {
    assert_eq!(
        occurrence_txn_merchant_name(world).as_deref(),
        Some(expected.as_str()),
        "流水商户名不符（左联 merchants 现名）"
    );
}

/// 迁移后 schema 就位：installment/subscription 扩展表含 merchant_id 列、无 counterparty 列
/// （issue #190 / ADR-0028：counterparty 文本列原地改为商户引用，不写前向迁移）。
#[then(expr = "计划扩展表应含 merchant_id 列且无 counterparty 列")]
fn assert_scheduled_ext_schema(world: &mut LedgerWorld) {
    for table in ["installment_plans", "subscription_plans"] {
        let merchant: i64 = world
            .conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name='merchant_id'",
                params![table],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(merchant, 1, "{table} 应含 merchant_id 列");
        let counterparty: i64 = world
            .conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name='counterparty'",
                params![table],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(counterparty, 0, "{table} 不应再含 counterparty 列");
    }
}

/// 最近计划生成的每笔交易商户名都应是指定商户（分期逐期断言）。
#[then(expr = "最近计划生成的每笔交易商户应为 {string}")]
fn assert_all_plan_txns_merchant(world: &mut LedgerWorld, expected: String) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let names: Vec<Option<String>> = {
        let mut stmt = world
            .conn
            .prepare(
                "SELECT m.name FROM transactions t \
                 JOIN scheduled_transaction_occurrences o ON o.transaction_id=t.id \
                 LEFT JOIN merchants m ON m.id = t.merchant_id \
                 WHERE o.scheduled_transaction_id=?1 AND o.is_deleted=0 \
                 ORDER BY t.date ASC, t.created_at ASC",
            )
            .unwrap();
        stmt.query_map(params![plan_id], |r| r.get::<_, Option<String>>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    };
    assert!(!names.is_empty(), "计划应已生成交易");
    for name in names {
        assert_eq!(name.as_deref(), Some(expected.as_str()), "计划交易商户不符");
    }
}

/// 最近创建的计划商户名（左联 merchants 现名）。
#[then(expr = "最近创建的计划商户应为 {string}")]
fn assert_plan_merchant(world: &mut LedgerWorld, expected: String) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let name: Option<String> = world
        .conn
        .query_row(
            "SELECT m.name FROM scheduled_transactions st \
             LEFT JOIN installment_plans ip ON ip.scheduled_transaction_id = st.id \
             LEFT JOIN subscription_plans sp ON sp.scheduled_transaction_id = st.id \
             LEFT JOIN merchants m ON m.id = COALESCE(ip.merchant_id, sp.merchant_id) \
             WHERE st.id=?1",
            params![plan_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .unwrap();
    assert_eq!(
        name.as_deref(),
        Some(expected.as_str()),
        "计划商户名不符（左联 merchants 现名）"
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
            merchant_id: None,
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

/// 编辑最近订阅计划的备注/分类（走 update_subscription 命令体，账户不变）。
#[when(expr = "编辑该订阅计划 备注 {string} 分类 {string}")]
fn edit_subscription_plan(world: &mut LedgerWorld, note: String, category: String) {
    edit_subscription_plan_inner(world, note, Some(category), None);
}

/// 编辑最近订阅计划的商户（issue #190：改商户只影响未来期次；其余字段取当前值）。
#[when(expr = "编辑该订阅计划 商户 {string}")]
fn edit_subscription_plan_merchant(world: &mut LedgerWorld, merchant: String) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let (account_id, category_id, note): (String, Option<String>, Option<String>) = world
        .conn
        .query_row(
            "SELECT account_id,category_id,note FROM scheduled_transactions WHERE id=?1",
            params![plan_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    update_subscription(
        &world.conn,
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
    .expect("编辑订阅商户失败");
}

/// 最近计划生成的第 n 笔交易商户名（左联 merchants 现名）。
#[then(expr = "第 {int} 笔计划交易商户应为 {string}")]
fn assert_plan_txn_merchant(world: &mut LedgerWorld, nth: usize, expected: String) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let name: Option<String> = world
        .conn
        .query_row(
            "SELECT m.name FROM transactions t \
             JOIN scheduled_transaction_occurrences o ON o.transaction_id=t.id \
             LEFT JOIN merchants m ON m.id = t.merchant_id \
             WHERE o.scheduled_transaction_id=?1 AND o.is_deleted=0 \
             ORDER BY t.date ASC, t.created_at ASC LIMIT 1 OFFSET ?2",
            params![plan_id, (nth - 1) as i64],
            |r| r.get::<_, Option<String>>(0),
        )
        .unwrap_or_else(|e| panic!("计划应已生成第 {nth} 笔交易: {e}"));
    assert_eq!(
        name.as_deref(),
        Some(expected.as_str()),
        "第 {nth} 笔计划交易商户不符"
    );
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
    let (current_account_id, current_category_id): (String, Option<String>) = world
        .conn
        .query_row(
            "SELECT account_id,category_id FROM scheduled_transactions WHERE id=?1",
            params![plan_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    // 商户为全量替换语义：合法编辑补齐当前商户（含软删商户保持历史引用）。
    let current_merchant: Option<String> = world
        .conn
        .query_row(
            "SELECT merchant_id FROM subscription_plans WHERE scheduled_transaction_id=?1",
            params![plan_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .unwrap();
    let category_id = match &category {
        Some(name) => Some(
            category_id_by_name(&world.conn, name)
                .unwrap_or_else(|| panic!("支出分类 '{name}' 不存在")),
        ),
        None => current_category_id,
    };
    update_subscription(
        &world.conn,
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
    .expect("编辑订阅计划失败");
}

/// 携带金额字段发出编辑请求：应被后端显式拒绝（ADR-0023 决策三）。
#[when(expr = "携带金额 {int} 编辑该订阅计划")]
fn edit_subscription_plan_with_amount(world: &mut LedgerWorld, _amount: i64) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let (account_id, category_id, note): (String, Option<String>, Option<String>) = world
        .conn
        .query_row(
            "SELECT account_id,category_id,note FROM scheduled_transactions WHERE id=?1",
            params![plan_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    let merchant_id: Option<String> = world
        .conn
        .query_row(
            "SELECT merchant_id FROM subscription_plans WHERE scheduled_transaction_id=?1",
            params![plan_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .unwrap();
    match update_subscription(
        &world.conn,
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
    account_id: String,
}

fn plan_generated_txn(world: &LedgerWorld, nth: usize) -> PlanTxnRow {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    world
        .conn
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
    let account_id: String = world
        .conn
        .query_row(
            "SELECT account_id FROM scheduled_transactions WHERE id=?1",
            params![plan_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(account_id, world.account_id(&expected), "计划扣款账户不符");
}
