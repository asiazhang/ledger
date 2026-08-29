//! 创建定时计划：订阅 / 分期 / 定时转账变体（含无限循环、币种不一致拒绝）。

use cucumber::when;
use rusqlite::params;

use tauri_app_lib::error::AppError;
use tauri_app_lib::scheduled_transactions::{
    CreateScheduledInput, RecurrenceType, ScheduledKind, create_plan,
};

use crate::world::LedgerWorld;

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
    let id = world
        .db
        .write(|conn| {
            create_plan(
                conn,
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
        })
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
    let id = world
        .db
        .write(|conn| {
            create_plan(
                conn,
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
        })
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
    let id = world
        .db
        .write(|conn| {
            create_plan(
                conn,
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
        })
        .expect("创建定时转账计划失败");
    world.last_plan_id = Some(id);
}

/// 创建不带期数的定时转账（无限循环，total_occurrences=None）并记录 id（issue #203）。
#[when(expr = "创建定时转账计划 金额 {int} 从 {string} 到 {string} 起始日期 {string}")]
fn create_scheduled_transfer_plan_infinite(
    world: &mut LedgerWorld,
    amount: i64,
    from: String,
    to: String,
    start: String,
) {
    // 计划币种取转出账户实际币种（同币种校验比的是两账户，不比提交值）
    let currency: String = world_conn!(world)
        .query_row(
            "SELECT currency_code FROM accounts WHERE id=?1",
            params![world.account_id(&from)],
            |r| r.get(0),
        )
        .unwrap();
    let id = world
        .db
        .write(|conn| {
            create_plan(
                conn,
                CreateScheduledInput {
                    kind: ScheduledKind::ScheduledTransfer,
                    account_id: world.account_id(&from),
                    category_id: None,
                    amount_cents: amount,
                    currency_code: currency,
                    recurrence_type: RecurrenceType::Monthly,
                    recurrence_interval: 1,
                    recurrence_day: None,
                    start_date: start,
                    note: None,
                    merchant_id: None,
                    total_amount_cents: None,
                    total_occurrences: None,
                    to_account_id: Some(world.account_id(&to)),
                },
            )
        })
        .expect("创建定时转账计划失败");
    world.last_plan_id = Some(id);
}

/// 尝试创建定时转账计划（不带商户）并捕获错误：两账户币种不一致被拒（issue #203）。
#[when(
    expr = "尝试创建定时转账计划 金额 {int} 从 {string} 到 {string} 期数 {int} 起始日期 {string}"
)]
fn try_create_transfer_plan(
    world: &mut LedgerWorld,
    amount: i64,
    from: String,
    to: String,
    occurrences: i64,
    start: String,
) {
    // 计划币种取转出账户实际币种：不硬编码，避免币种与账户不符的隐蔽数据
    let currency: String = world_conn!(world)
        .query_row(
            "SELECT currency_code FROM accounts WHERE id=?1",
            params![world.account_id(&from)],
            |r| r.get(0),
        )
        .unwrap();
    let result = world.db.write(|conn| {
        create_plan(
            conn,
            CreateScheduledInput {
                kind: ScheduledKind::ScheduledTransfer,
                account_id: world.account_id(&from),
                category_id: None,
                amount_cents: amount,
                currency_code: currency,
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
    });
    world.last_error = match result {
        Err(AppError::Invalid(msg)) => Some(msg),
        Ok(_) => None,
        Err(e) => Some(e.to_string()),
    };
}
