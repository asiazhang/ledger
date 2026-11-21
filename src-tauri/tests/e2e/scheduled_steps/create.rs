use cucumber::when;

use ledger_infra::error::AppError;
use ledger_scheduled::CreateScheduledInput;

use crate::step_inputs::{
    installment_plan_input, scheduled_transfer_plan_input, subscription_plan_input,
};
use crate::step_verbs::{self, account_currency_code, create_plan_verb, try_create_plan_verb};
use crate::world::LedgerWorld;

#[when(expr = "创建订阅计划 金额 {int} 币种 {string} 账户 {string} 起始日期 {string}")]
fn create_subscription_plan(
    world: &mut LedgerWorld,
    amount: i64,
    currency: String,
    account: String,
    start: String,
) {
    step_verbs::create_subscription_plan(world, amount, &currency, &account, &start);
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
    let account_id = world.account_id(&account);
    create_plan_verb(
        world,
        CreateScheduledInput {
            note: Some(note),
            ..subscription_plan_input(amount, &account_id, &currency, &start)
        },
    );
}

#[when(expr = "创建分期计划 总额 {int} 期数 {int} 账户 {string} 起始日期 {string}")]
fn create_installment_plan(
    world: &mut LedgerWorld,
    total: i64,
    occurrences: i64,
    account: String,
    start: String,
) {
    step_verbs::create_installment_plan(world, total, occurrences, &account, &start);
}

#[when(expr = "创建分期计划 总额 {int} 期数 {int} 账户 {string} 起始日期 {string} 备注 {string}")]
fn create_installment_plan_with_note(
    world: &mut LedgerWorld,
    total: i64,
    occurrences: i64,
    account: String,
    start: String,
    note: String,
) {
    let account_id = world.account_id(&account);
    let currency = account_currency_code(world, &account_id);
    create_plan_verb(
        world,
        CreateScheduledInput {
            note: Some(note),
            ..installment_plan_input(total, occurrences, &account_id, &currency, &start)
        },
    );
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
    step_verbs::create_scheduled_transfer_plan(
        world,
        amount,
        &from,
        &to,
        Some(occurrences),
        &start,
    );
}

#[when(
    expr = "创建定时转账计划 金额 {int} 从 {string} 到 {string} 期数 {int} 起始日期 {string} 备注 {string}"
)]
fn create_scheduled_transfer_plan_with_note(
    world: &mut LedgerWorld,
    amount: i64,
    from: String,
    to: String,
    occurrences: i64,
    start: String,
    note: String,
) {
    let from_id = world.account_id(&from);
    let to_id = world.account_id(&to);
    let currency = account_currency_code(world, &from_id);
    create_plan_verb(
        world,
        CreateScheduledInput {
            total_occurrences: Some(occurrences),
            note: Some(note),
            ..scheduled_transfer_plan_input(amount, &from_id, &to_id, &currency, &start)
        },
    );
}

#[when(expr = "创建定时转账计划 金额 {int} 从 {string} 到 {string} 起始日期 {string}")]
fn create_scheduled_transfer_plan_infinite(
    world: &mut LedgerWorld,
    amount: i64,
    from: String,
    to: String,
    start: String,
) {
    step_verbs::create_scheduled_transfer_plan(world, amount, &from, &to, None, &start);
}

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
    let from_id = world.account_id(&from);
    let to_id = world.account_id(&to);
    let currency = account_currency_code(world, &from_id);
    let input = CreateScheduledInput {
        total_occurrences: Some(occurrences),
        ..scheduled_transfer_plan_input(amount, &from_id, &to_id, &currency, &start)
    };
    let result = try_create_plan_verb(world, input);
    world.last_error = match result {
        Err(AppError::Invalid(msg)) => Some(msg),
        Ok(_) => None,
        Err(e) => Some(e.to_string()),
    };
}
