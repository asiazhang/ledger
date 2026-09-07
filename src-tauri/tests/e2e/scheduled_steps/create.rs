//! 创建定时计划：订阅 / 分期 / 定时转账变体（含无限循环、币种不一致拒绝）。
//!
//! #762 迁移：输入构造收编 L1 步骤输入工厂（[`crate::step_inputs`] 三形态工厂）、
//! 写入收编 L2 步骤动词（[`crate::step_verbs`] 计划动词，经计划域公开创建入口
//! `create_plan`，非 IPC 命令、非裸 SQL）；步骤函数薄化为名称解析 + 冷字段覆盖。
//! 币种口径：订阅随步骤文本显式给出（计划行原样存储）；分期/转账取账户实际币种
//! （与既有 CNY 硬编码在 CNY 账户场景等价，且不引入「币种与账户不符」的隐蔽数据）。

use cucumber::when;

use tauri_app_lib::error::AppError;
use tauri_app_lib::scheduled_transactions::CreateScheduledInput;

use crate::step_inputs::{
    installment_plan_input, scheduled_transfer_plan_input, subscription_plan_input,
};
use crate::step_verbs::{self, account_currency_code, create_plan_verb, try_create_plan_verb};
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
    step_verbs::create_subscription_plan(world, amount, &currency, &account, &start);
}

/// 带备注的订阅计划变体：备注为冷字段，L1 工厂 + 结构体更新覆盖。
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

/// 带备注的分期计划变体（issue #707 来源列场景）：备注即计划名（来源列展示名口径）。
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

/// 带备注的定时转账计划变体（issue #707 来源列场景）：备注即计划名。
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

/// 创建不带期数的定时转账（无限循环，total_occurrences=None）并记录 id（issue #203）。
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
    let from_id = world.account_id(&from);
    let to_id = world.account_id(&to);
    // 计划币种取转出账户实际币种：不硬编码，避免币种与账户不符的隐蔽数据
    let currency = account_currency_code(world, &from_id);
    let input = CreateScheduledInput {
        total_occurrences: Some(occurrences),
        ..scheduled_transfer_plan_input(amount, &from_id, &to_id, &currency, &start)
    };
    let result = try_create_plan_verb(world, input);
    // 断言语义保持原样（区别于「预期失败但成功了」捕获：成功记 None 清空错误）
    world.last_error = match result {
        Err(AppError::Invalid(msg)) => Some(msg),
        Ok(_) => None,
        Err(e) => Some(e.to_string()),
    };
}
