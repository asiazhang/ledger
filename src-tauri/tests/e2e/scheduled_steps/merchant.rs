//! 带商户的定时计划（issue #190 / ADR-0028）：installment/subscription 可携带商户、
//! 定时转账拒绝携带；商户相关断言（计划 / 生成流水 / 扩展表 schema）也归此。
//!
//! #762 迁移：输入构造收编 L1 三形态工厂（[`crate::step_inputs`]，商户为冷字段
//! 覆盖）、写入收编 L2 计划动词（[`crate::step_verbs`]）；try 形态 +
//! `capture_expected_error` 承接「应返回错误」断言，断言语义不变。

use cucumber::{then, when};
use rusqlite::params;

use tauri_app_lib::scheduled_transactions::CreateScheduledInput;

use crate::common::capture_expected_error;
use crate::step_inputs::{
    installment_plan_input, scheduled_transfer_plan_input, subscription_plan_input,
};
use crate::step_verbs::{account_currency_code, create_plan_verb, try_create_plan_verb};
use crate::world::LedgerWorld;

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
    let account_id = world.account_id(&account);
    create_plan_verb(
        world,
        CreateScheduledInput {
            note: Some(note),
            merchant_id: Some(world.merchant_id(&merchant)),
            ..subscription_plan_input(amount, &account_id, &currency, &start)
        },
    );
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
    let account_id = world.account_id(&account);
    let currency = account_currency_code(world, &account_id);
    create_plan_verb(
        world,
        CreateScheduledInput {
            merchant_id: Some(world.merchant_id(&merchant)),
            ..installment_plan_input(total, occurrences, &account_id, &currency, &start)
        },
    );
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
    let from_id = world.account_id(&from);
    let to_id = world.account_id(&to);
    let currency = account_currency_code(world, &from_id);
    let input = CreateScheduledInput {
        merchant_id: Some(world.merchant_id(&merchant)),
        total_occurrences: Some(occurrences),
        ..scheduled_transfer_plan_input(amount, &from_id, &to_id, &currency, &start)
    };
    let result = try_create_plan_verb(world, input);
    capture_expected_error(world, result);
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
    let account_id = world.account_id(&account);
    let input = CreateScheduledInput {
        merchant_id: Some(world.merchant_id(&merchant)),
        ..subscription_plan_input(amount, &account_id, &currency, &start)
    };
    let result = try_create_plan_verb(world, input);
    capture_expected_error(world, result);
}

/// 最近期次生成的交易商户名（左联 merchants 现名：改名即时生效，软删照常显示）。
fn occurrence_txn_merchant_name(world: &LedgerWorld) -> Option<String> {
    let occ_id = world.plan.last_occurrence_id.clone().expect("尚无期次");
    world_conn!(world)
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
        let merchant: i64 = world_conn!(world)
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name='merchant_id'",
                params![table],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(merchant, 1, "{table} 应含 merchant_id 列");
        let counterparty: i64 = world_conn!(world)
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
    let plan_id = world.plan.last_plan_id.clone().expect("尚无定时计划");
    let names: Vec<Option<String>> = {
        let conn = world_conn!(world);
        let mut stmt = conn
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
    let plan_id = world.plan.last_plan_id.clone().expect("尚无定时计划");
    let name: Option<String> = world_conn!(world)
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

/// 最近计划生成的第 n 笔交易商户名（左联 merchants 现名）。
#[then(expr = "第 {int} 笔计划交易商户应为 {string}")]
fn assert_plan_txn_merchant(world: &mut LedgerWorld, nth: usize, expected: String) {
    let plan_id = world.plan.last_plan_id.clone().expect("尚无定时计划");
    let name: Option<String> = world_conn!(world)
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
