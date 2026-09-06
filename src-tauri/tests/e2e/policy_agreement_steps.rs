//! 保单缴费协议（issue #362 / spec #358 / ADR-0051 决策 2）BDD 步骤：
//! 经保单上下文创建订阅形态协议（持保单引用）、期次生成流水的引用复制断言、
//! 多段协议历史断言、准入守卫（只有订阅形态可挂保单）。
//!
//! 「为最近保单创建缴费协议」= 前端保单弹窗协议区的写路径（两次 IPC 组合：
//! `create_policy` 已由既有步骤完成，本步骤直调 `create_plan` 携带 `policy_id`），
//! 与 transactions_policy_steps 直调行为层 seam 的先例一致。
//! 期次执行与计划状态变更复用 `scheduled_steps` 已注册步骤。

use cucumber::{then, when};
use rusqlite::params;

use tauri_app_lib::scheduled_transactions::{
    CreateScheduledInput, RecurrenceType, ScheduledKind, UpdateSubscriptionInput, create_plan,
    update_subscription,
};

use crate::world::LedgerWorld;

/// 周期字符串 → RecurrenceType（BDD 文案用小写枚举名，与 CreateScheduledInput 一致）。
fn parse_recurrence(s: &str) -> RecurrenceType {
    match s {
        "daily" => RecurrenceType::Daily,
        "weekly" => RecurrenceType::Weekly,
        "monthly" => RecurrenceType::Monthly,
        "yearly" => RecurrenceType::Yearly,
        other => panic!("未知周期类型: {other}"),
    }
}

// ---------------------------------------------------------------------------
// When：经保单上下文创建缴费协议（订阅形态 + 保单引用）
// ---------------------------------------------------------------------------

/// 为最近创建的保单创建缴费协议并记录计划 id（要求成功）。
/// 携带的 `policy_id` 即「协议 → 保单」引用（V014 列），期次生成流水时复制。
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
    let policy_id = world.last_policy_id.clone().expect("尚无保单");
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
    world.last_plan_id = Some(id);
}

/// 尝试为最近创建的保单创建缴费协议并捕获错误（软删保单不可被新协议选择）。
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
    let policy_id = world.last_policy_id.clone().expect("尚无保单");
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

/// 尝试为最近创建的保单创建携带商户的缴费协议并捕获错误
/// （守卫：保单协议不挂商户，ADR-0082 决策 2）。
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
    let policy_id = world.last_policy_id.clone().expect("尚无保单");
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

/// 保单缴费协议创建共用组装（订阅形态 + 保单引用；`merchant_id` 由调用方决定——
/// 合法路径恒 `None`，守卫场景显式携带以验证拒绝）。
#[allow(clippy::too_many_arguments)] // 步骤参数全量透传，无法缩减
fn create_policy_plan(
    world: &mut LedgerWorld,
    policy_id: &str,
    amount: i64,
    currency: &str,
    account: &str,
    recurrence: &str,
    start: &str,
    merchant_id: Option<String>,
) -> tauri_app_lib::error::Result<String> {
    world.db.write(|conn| {
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

/// 尝试编辑最近创建的保单缴费协议计划并提交商户（捕获错误：挂保单计划行
/// 不回挂商户，ADR-0082 决策 2）。
#[when(expr = "尝试编辑该订阅计划 商户 {string}")]
fn try_edit_policy_plan_merchant(world: &mut LedgerWorld, merchant: String) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let (account_id, category_id, note): (String, Option<String>, Option<String>) =
        world_conn!(world)
            .query_row(
                "SELECT account_id,category_id,note FROM scheduled_transactions WHERE id=?1",
                params![plan_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
    let merchant_id = world.merchant_id(&merchant);
    let result = world.db.write(|conn| {
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

/// 尝试创建携带保单的分期计划并捕获错误（准入守卫：只有订阅形态可挂保单）。
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
    // 保单 id 解析在写闭包外：policy_id_by_number 内部取连接锁，闭包内调用会自锁死锁。
    let policy_id = policy_id_by_number(world, &policy_number);
    let result = world.db.write(|conn| {
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

/// 尝试创建携带保单的定时转账计划并捕获错误（准入守卫：只有订阅形态可挂保单）。
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
    // 保单 id 解析在写闭包外（同上，避免闭包内重入连接锁）。
    let policy_id = policy_id_by_number(world, &policy_number);
    let result = world.db.write(|conn| {
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

/// 按保单号查保单 id（**含已软删**——历史引用断言依赖软删行仍可定位；
/// 保单号在各场景内唯一，无歧义）。
fn policy_id_by_number(world: &LedgerWorld, policy_number: &str) -> String {
    world_conn!(world)
        .query_row(
            "SELECT id FROM policies WHERE policy_number=?1",
            params![policy_number],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| panic!("保单 '{}' 不存在", policy_number))
}

// ---------------------------------------------------------------------------
// Then：期次流水引用复制 / 多段协议历史
// ---------------------------------------------------------------------------

/// 最近执行期次生成的交易不应携带商户引用（保费不挂商户，ADR-0082 决策 2：
/// 计划行商户置空/不写，期次对空商户透传——归属唯一事实是 policy_id）。
#[then(expr = "该期次交易不应携带商户")]
fn assert_occurrence_txn_without_merchant(world: &mut LedgerWorld) {
    let occ_id = world.last_occurrence_id.clone().expect("尚无期次");
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

/// 最近执行期次生成的交易挂单应为指定保单（按保单号定位）。
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

/// 最近计划已生成（期次回填）的全部交易均挂同一保单。
#[then(expr = "最近计划生成的每笔交易挂单均应为保单号 {string}")]
fn assert_plan_txns_all_policy(world: &mut LedgerWorld, policy_number: String) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
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

/// 最近保单名下的协议段数（含已取消/暂停——多段历史 = 费率变更的分段真相）。
#[then(expr = "最近保单的协议历史应有 {int} 段")]
fn assert_policy_plan_segment_count(world: &mut LedgerWorld, expected: i64) {
    assert_eq!(
        policy_plan_rows(world).len() as i64,
        expected,
        "保单协议历史段数不符"
    );
}

/// 最近保单第 `n` 段协议（按创建先后）的状态与每期金额。
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

/// 最近执行期次生成交易的 policy_id（未回填则 panic，与 occurrence 断言先例一致）。
fn occurrence_txn_policy_id(world: &LedgerWorld) -> Option<String> {
    let occ_id = world.last_occurrence_id.clone().expect("尚无期次");
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

/// 协议历史行（状态 + 每期金额），按创建先后排序。
/// 排序键 `created_at, rowid`：`now_iso` 为秒级精度，同场景连建两段协议
/// `created_at` 相同，以插入顺序（rowid）稳定分段次。
struct PolicyPlanRow {
    status: String,
    amount_cents: i64,
}

fn policy_plan_rows(world: &LedgerWorld) -> Vec<PolicyPlanRow> {
    let policy_id = world.last_policy_id.clone().expect("尚无保单");
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
