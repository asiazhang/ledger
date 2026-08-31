//! 执行期次与引擎落库断言：汇率夹具、期次执行（含 #230 事务自持注入）、
//! 生成交易的类型 / 金额 / 状态断言。

use cucumber::{given, then, when};
use rusqlite::params;

use crate::common::assert_last_error_contains;
use crate::world::LedgerWorld;

use super::common::execute_occurrence_step;

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

/// 写入一条汇率（base → quote）。币种须存在于种子 currencies（FK 约束）。
#[given(expr = "存在汇率 {string} 兑 {string} 为 {float}")]
fn add_exchange_rate(world: &mut LedgerWorld, base: String, quote: String, rate: f64) {
    world_conn!(world)
        .execute(
            "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,updated_at,version,device_id) \
             VALUES ('er-' || hex(randomblob(8)), ?1, ?2, ?3, '2026-02-01T00:00:00Z','2026-02-01T00:00:00Z',1,'test')",
            params![base, quote, rate],
        )
        .unwrap();
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
    let conn = world_conn!(world);
    let mut stmt = conn
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

// ---------------------------------------------------------------------------
// 引擎事务自持：期次中间态缺口修复（issue #230 / ADR-0033 决策 #6）
// ---------------------------------------------------------------------------

/// 注入「期次落库中途失败」：期次已 CAS 置 processing 后，交易行 INSERT 被触发器
/// RAISE(ABORT) 挡下——纯测试侧注入（spec #169 定案），产品代码零 hook。
#[when(expr = "注入交易落库失败触发器")]
fn inject_txn_insert_failure(world: &mut LedgerWorld) {
    world_conn!(world)
        .execute(
            "CREATE TRIGGER block_txn_insert BEFORE INSERT ON transactions \
             BEGIN SELECT RAISE(ABORT, '测试注入：期次落库失败'); END",
            [],
        )
        .unwrap();
}

/// 移除注入触发器，让回滚后回原状态的期次可重试。
#[when(expr = "移除交易落库失败触发器")]
fn drop_txn_insert_failure_trigger(world: &mut LedgerWorld) {
    world_conn!(world)
        .execute("DROP TRIGGER block_txn_insert", [])
        .unwrap();
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
    let txn_id: Option<String> = world_conn!(world)
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
    let status: String = world_conn!(world)
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
        let conn = world_conn!(world);
        let mut stmt = conn
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
    let status: String = world_conn!(world)
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
