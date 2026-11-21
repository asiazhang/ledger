use cucumber::{then, when};
use rusqlite::params;

use ledger_scheduled::CreateScheduledInput;

use crate::common::capture_expected_error;
use crate::step_inputs::{
    installment_plan_input, scheduled_transfer_plan_input, subscription_plan_input,
};
use crate::step_verbs::{account_currency_code, create_plan_verb, try_create_plan_verb};
use crate::world::LedgerWorld;

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
