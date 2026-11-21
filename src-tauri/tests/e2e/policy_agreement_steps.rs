use cucumber::{then, when};
use rusqlite::params;

use ledger_scheduled::{
    CreateScheduledInput, RecurrenceType, ScheduledKind, UpdateSubscriptionInput, create_plan,
    update_subscription,
};

use crate::world::LedgerWorld;

fn parse_recurrence(s: &str) -> RecurrenceType {
    match s {
        "daily" => RecurrenceType::Daily,
        "weekly" => RecurrenceType::Weekly,
        "monthly" => RecurrenceType::Monthly,
        "yearly" => RecurrenceType::Yearly,
        other => panic!("未知周期类型: {other}"),
    }
}

#[when(
    expr = "为最近保单创建缴费协议 金额 {int} 币种 {string} 账户 {string} 周期 {string} 起始日期 {string}"
)]
fn create_policy_agreement(
    world: &mut LedgerWorld,
    amount: i64,
    currency: String,
    account: String,
    recurrence: String,
    start: String,
) {
    let policy_id = world.policy.last_policy_id.clone().expect("尚无保单");
    let id = world_write!(world, |conn| {
        create_plan(
            conn,
            CreateScheduledInput {
                kind: ScheduledKind::Subscription,
                account_id: world.account_id(&account),
                category_id: None,
                amount_cents: amount,
                currency_code: currency,
                recurrence_type: parse_recurrence(&recurrence),
                recurrence_interval: 1,
                recurrence_day: None,
                start_date: start,
                note: None,
                merchant_id: None,
                policy_id: Some(policy_id),
                total_amount_cents: None,
                total_occurrences: None,
                to_account_id: None,
            },
        )
    })
    .expect("创建保单缴费协议应成功但失败");
    world.plan.last_plan_id = Some(id);
}

#[when(
    expr = "尝试为最近保单创建缴费协议 金额 {int} 币种 {string} 账户 {string} 周期 {string} 起始日期 {string}"
)]
fn try_create_policy_agreement(
    world: &mut LedgerWorld,
    amount: i64,
    currency: String,
    account: String,
    recurrence: String,
    start: String,
) {
    let policy_id = world.policy.last_policy_id.clone().expect("尚无保单");
    let result = create_policy_plan(
        world,
        &policy_id,
        amount,
        &currency,
        &account,
        &recurrence,
        &start,
        None,
    );
    match result {
        Ok(_) => panic!("创建缴费协议应失败但成功"),
        Err(e) => world.last_error = Some(e.to_string()),
    }
}

#[when(
    expr = "尝试为最近保单创建缴费协议 金额 {int} 币种 {string} 账户 {string} 周期 {string} 起始日期 {string} 带商户 {string}"
)]
fn try_create_policy_agreement_with_merchant(
    world: &mut LedgerWorld,
    amount: i64,
    currency: String,
    account: String,
    recurrence: String,
    start: String,
    merchant: String,
) {
    let policy_id = world.policy.last_policy_id.clone().expect("尚无保单");
    let merchant_id = world.merchant_id(&merchant);
    let result = create_policy_plan(
        world,
        &policy_id,
        amount,
        &currency,
        &account,
        &recurrence,
        &start,
        Some(merchant_id),
    );
    match result {
        Ok(_) => panic!("创建缴费协议应失败但成功"),
        Err(e) => world.last_error = Some(e.to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
fn create_policy_plan(
    world: &mut LedgerWorld,
    policy_id: &str,
    amount: i64,
    currency: &str,
    account: &str,
    recurrence: &str,
    start: &str,
    merchant_id: Option<String>,
) -> ledger_infra::error::Result<String> {
    world_write!(world, |conn| {
        create_plan(
            conn,
            CreateScheduledInput {
                kind: ScheduledKind::Subscription,
                account_id: world.account_id(account),
                category_id: None,
                amount_cents: amount,
                currency_code: currency.to_string(),
                recurrence_type: parse_recurrence(recurrence),
                recurrence_interval: 1,
                recurrence_day: None,
                start_date: start.to_string(),
                note: None,
                merchant_id,
                policy_id: Some(policy_id.to_string()),
                total_amount_cents: None,
                total_occurrences: None,
                to_account_id: None,
            },
        )
    })
}

#[when(expr = "尝试编辑该订阅计划 商户 {string}")]
fn try_edit_policy_plan_merchant(world: &mut LedgerWorld, merchant: String) {
    let plan_id = world.plan.last_plan_id.clone().expect("尚无定时计划");
    let (account_id, category_id, note): (String, Option<String>, Option<String>) =
        world_conn!(world)
            .query_row(
                "SELECT account_id,category_id,note FROM scheduled_transactions WHERE id=?1",
                params![plan_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
    let merchant_id = world.merchant_id(&merchant);
    let result = world_write!(world, |conn| {
        update_subscription(
            conn,
            UpdateSubscriptionInput {
                id: plan_id,
                account_id,
                category_id,
                note,
                merchant_id: Some(merchant_id),
                amount_cents: false,
                total_amount_cents: false,
            },
        )
    });
    match result {
        Ok(()) => panic!("编辑保单协议计划应失败但成功"),
        Err(e) => world.last_error = Some(e.to_string()),
    }
}

#[when(
    expr = "尝试创建分期计划 总额 {int} 期数 {int} 账户 {string} 起始日期 {string} 挂保单 {string}"
)]
fn try_create_installment_with_policy(
    world: &mut LedgerWorld,
    total: i64,
    occurrences: i64,
    account: String,
    start: String,
    policy_number: String,
) {
    let policy_id = policy_id_by_number(world, &policy_number);
    let result = world_write!(world, |conn| {
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
                policy_id: Some(policy_id),
                total_amount_cents: Some(total),
                total_occurrences: Some(occurrences),
                to_account_id: None,
            },
        )
    });
    match result {
        Ok(_) => panic!("创建分期计划应失败但成功"),
        Err(e) => world.last_error = Some(e.to_string()),
    }
}

#[when(
    expr = "尝试创建定时转账计划 金额 {int} 从账户 {string} 到账户 {string} 期数 {int} 起始日期 {string} 挂保单 {string}"
)]
fn try_create_transfer_with_policy(
    world: &mut LedgerWorld,
    amount: i64,
    from: String,
    to: String,
    occurrences: i64,
    start: String,
    policy_number: String,
) {
    let policy_id = policy_id_by_number(world, &policy_number);
    let result = world_write!(world, |conn| {
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
                policy_id: Some(policy_id),
                total_amount_cents: None,
                total_occurrences: Some(occurrences),
                to_account_id: Some(world.account_id(&to)),
            },
        )
    });
    match result {
        Ok(_) => panic!("创建定时转账计划应失败但成功"),
        Err(e) => world.last_error = Some(e.to_string()),
    }
}

fn policy_id_by_number(world: &LedgerWorld, policy_number: &str) -> String {
    world_conn!(world)
        .query_row(
            "SELECT id FROM policies WHERE policy_number=?1",
            params![policy_number],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| panic!("保单 '{}' 不存在", policy_number))
}

#[then(expr = "该期次交易不应携带商户")]
fn assert_occurrence_txn_without_merchant(world: &mut LedgerWorld) {
    let occ_id = world.plan.last_occurrence_id.clone().expect("尚无期次");
    let (merchant_id, policy_id): (Option<String>, Option<String>) = world_conn!(world)
        .query_row(
            "SELECT t.merchant_id, t.policy_id FROM transactions t \
             JOIN scheduled_transaction_occurrences o ON o.transaction_id=t.id WHERE o.id=?1",
            params![occ_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(merchant_id.is_none(), "保费流水不应携带商户引用");
    assert!(policy_id.is_some(), "保费流水应挂保单（对照断言）");
}

#[then(expr = "该期次交易挂单应为保单号 {string}")]
fn assert_occurrence_txn_policy(world: &mut LedgerWorld, policy_number: String) {
    let policy_id = policy_id_by_number(world, &policy_number);
    let txn_policy: Option<String> = occurrence_txn_policy_id(world);
    assert_eq!(
        txn_policy.as_deref(),
        Some(policy_id.as_str()),
        "期次生成交易应挂保单 {policy_number}"
    );
}

#[then(expr = "最近计划生成的每笔交易挂单均应为保单号 {string}")]
fn assert_plan_txns_all_policy(world: &mut LedgerWorld, policy_number: String) {
    let plan_id = world.plan.last_plan_id.clone().expect("尚无定时计划");
    let policy_id = policy_id_by_number(world, &policy_number);
    let policies: Vec<Option<String>> = {
        let conn = world_conn!(world);
        let mut stmt = conn
            .prepare(
                "SELECT t.policy_id FROM transactions t \
                 JOIN scheduled_transaction_occurrences o ON o.transaction_id=t.id \
                 WHERE o.scheduled_transaction_id=?1 AND o.status='completed' AND o.is_deleted=0",
            )
            .unwrap();
        stmt.query_map(params![plan_id], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    };
    assert!(!policies.is_empty(), "计划应已有已完成的期次交易");
    for (i, p) in policies.iter().enumerate() {
        assert_eq!(
            p.as_deref(),
            Some(policy_id.as_str()),
            "第 {} 笔交易应挂保单 {policy_number}",
            i + 1
        );
    }
}

#[then(expr = "最近保单的协议历史应有 {int} 段")]
fn assert_policy_plan_segment_count(world: &mut LedgerWorld, expected: i64) {
    assert_eq!(
        policy_plan_rows(world).len() as i64,
        expected,
        "保单协议历史段数不符"
    );
}

#[then(expr = "最近保单第 {int} 段协议状态应为 {string} 每期金额应为 {int}")]
fn assert_policy_plan_segment(
    world: &mut LedgerWorld,
    n: usize,
    expected_status: String,
    expected_amount: i64,
) {
    let rows = policy_plan_rows(world);
    let row = rows
        .get(n - 1)
        .unwrap_or_else(|| panic!("保单协议历史不存在第 {n} 段"));
    assert_eq!(row.status, expected_status, "第 {n} 段协议状态不符");
    assert_eq!(
        row.amount_cents, expected_amount,
        "第 {n} 段协议每期金额不符"
    );
}

fn occurrence_txn_policy_id(world: &LedgerWorld) -> Option<String> {
    let occ_id = world.plan.last_occurrence_id.clone().expect("尚无期次");
    let txn_id: Option<String> = world_conn!(world)
        .query_row(
            "SELECT transaction_id FROM scheduled_transaction_occurrences WHERE id=?1",
            params![occ_id],
            |r| r.get(0),
        )
        .unwrap();
    let txn_id = txn_id.expect("期次尚未回填交易 id");
    world_conn!(world)
        .query_row(
            "SELECT policy_id FROM transactions WHERE id=?1",
            params![txn_id],
            |r| r.get(0),
        )
        .unwrap()
}

struct PolicyPlanRow {
    status: String,
    amount_cents: i64,
}

fn policy_plan_rows(world: &LedgerWorld) -> Vec<PolicyPlanRow> {
    let policy_id = world.policy.last_policy_id.clone().expect("尚无保单");
    let conn = world_conn!(world);
    let mut stmt = conn
        .prepare(
            "SELECT st.status, st.amount_cents FROM scheduled_transactions st \
             JOIN subscription_plans sp ON sp.scheduled_transaction_id=st.id \
             WHERE sp.policy_id=?1 AND st.is_deleted=0 \
             ORDER BY st.created_at, st.rowid",
        )
        .unwrap();
    stmt.query_map(params![policy_id], |r| {
        Ok(PolicyPlanRow {
            status: r.get(0)?,
            amount_cents: r.get(1)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}
