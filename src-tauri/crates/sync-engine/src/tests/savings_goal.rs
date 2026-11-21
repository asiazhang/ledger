use super::super::{DomainCommand, OpOutcome, ingest_ops, parked_ops, read_ops};
use super::common::{make_expense, seed_device, wire_in, wire_out};
use ledger_savings_goal::{
    SavingsGoalCommand, SavingsGoalInput, SavingsGoalUpdateInput, archive_savings_goal,
    create_savings_goal, delete_savings_goal, unarchive_savings_goal, update_savings_goal,
};
use ledger_transaction::TransactionInput;
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::write::protocol;
use tauri_app_lib::test_support;

fn goal_input(name: &str, amount_cents: i64) -> SavingsGoalInput {
    SavingsGoalInput {
        name: name.into(),
        target_amount_cents: amount_cents,
        deadline: None,
    }
}

fn make_income(account_id: &str, amount_cents: i64, note: &str) -> TransactionInput {
    TransactionInput {
        funding: Vec::new(),
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Income,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: Some(account_id.into()),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
        note: Some(note.into()),
        date: "2026-01-10".into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
        origin: None,
        fx_rate: None,
        purchases: Vec::new(),
    }
}

#[allow(clippy::type_complexity)]
fn read_goal(
    conn: &rusqlite::Connection,
    id: &str,
) -> Option<(String, i64, Option<String>, Option<i64>, String, i64)> {
    conn.query_row(
        "SELECT name,target_amount_cents,deadline,planned_monthly_cents,status,is_deleted \
         FROM goals WHERE id=?1",
        [id],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        },
    )
    .ok()
}

fn goal_ops(conn: &rusqlite::Connection) -> Vec<super::super::SyncOp> {
    read_ops(conn)
        .unwrap()
        .into_iter()
        .filter(|op| op.command.subject().0 == "goal")
        .collect()
}

#[test]
fn goal_lifecycle_ops_replay_and_converge() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    let id = create_savings_goal(&conn_a, &goal_input("买车", 100_000)).unwrap();
    update_savings_goal(
        &conn_a,
        &id,
        &SavingsGoalUpdateInput {
            name: "买车基金".into(),
            target_amount_cents: 120_000,
            deadline: Some("2027-12-31".into()),
            planned_monthly_cents: Some(10_000),
        },
    )
    .unwrap();
    archive_savings_goal(&conn_a, &id).unwrap();
    unarchive_savings_goal(&conn_a, &id).unwrap();

    let ops_on_a = goal_ops(&conn_a);
    assert_eq!(
        ops_on_a.len(),
        4,
        "创建/编辑/归档/取消归档各产出一条 op（删除即变红：移除任一写入口的 \
         record_local 接线，此断言即红）"
    );
    assert!(matches!(
        &ops_on_a[0].command,
        DomainCommand::Goal(SavingsGoalCommand::Create { .. })
    ));

    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(
        !reports
            .iter()
            .any(|r| matches!(r.outcome, OpOutcome::Parked { .. })),
        "目标 op 全部落地（夹具账户 op 重投递为 Skipped）：{reports:?}"
    );
    assert_eq!(
        read_goal(&conn_a, &id),
        read_goal(&conn_b, &id),
        "重放后目标状态一致"
    );
    let account_id: String = conn_a
        .query_row("SELECT account_id FROM goals WHERE id=?1", [&id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        conn_b
            .query_row(
                "SELECT name FROM accounts WHERE id=?1",
                [&account_id],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "买车基金",
        "改名联动的账户 op 先于目标编辑 op 到达并生效"
    );
    assert_eq!(
        read_ops(&conn_a).unwrap(),
        read_ops(&conn_b).unwrap(),
        "重放端零本地 op（两端日志逐条相等）"
    );

    delete_savings_goal(&conn_a, &id).unwrap();
    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(
        !reports
            .iter()
            .any(|r| matches!(r.outcome, OpOutcome::Parked { .. })),
        "删除 op 落地：{reports:?}"
    );
    assert_eq!(
        goal_ops(&conn_a).len(),
        5,
        "删除入口同样产出一条 op（删除即变红）"
    );
    assert_eq!(read_goal(&conn_a, &id), read_goal(&conn_b, &id));
    let account_alive: i64 = conn_b
        .query_row(
            "SELECT COUNT(*) FROM accounts WHERE id=?1 AND is_deleted=0",
            [&account_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(account_alive, 0, "级联软删专属账户经既有账户 op 收敛");
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
}

#[test]
fn goal_delete_nonzero_balance_parks_and_self_heals() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    let id = create_savings_goal(&conn_a, &goal_input("买车", 100_000)).unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));
    let account_id: String = conn_a
        .query_row("SELECT account_id FROM goals WHERE id=?1", [&id], |r| {
            r.get(0)
        })
        .unwrap();
    protocol::create(&conn_a, make_expense(&account_id, 500, "从目标池支出")).unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));

    assert!(delete_savings_goal(&conn_a, &id).is_err());
    assert_eq!(goal_ops(&conn_a).len(), 1, "本地被拒不产 op");

    protocol::create(&conn_a, make_income(&account_id, 500, "年终奖存入")).unwrap();
    delete_savings_goal(&conn_a, &id).unwrap();
    let full = wire_out(&conn_a);
    let goal_delete = full.last().unwrap().clone();

    let reports = wire_in(&conn_b, std::slice::from_ref(&goal_delete));
    assert!(
        matches!(
            &reports[0].outcome,
            OpOutcome::Parked { code, .. } if code == "savings-goal.delete-balance-nonzero"
        ),
        "重放侧禁删守卫与本地同码挂起：{reports:?}"
    );
    let parked = parked_ops(&conn_b).unwrap();
    assert_eq!(parked.len(), 1);
    assert_eq!(parked[0].entity, "goal");
    assert_eq!(
        read_goal(&conn_b, &id).unwrap().5,
        0,
        "禁删守卫拦下删除，目标行未软删"
    );

    let reports = wire_in(&conn_b, &full);
    assert!(
        reports
            .iter()
            .all(|r| matches!(r.outcome, OpOutcome::Applied | OpOutcome::Skipped)),
        "补齐依赖后目标删除 op 落地：{reports:?}"
    );
    assert!(parked_ops(&conn_b).unwrap().is_empty(), "成功即出队");
    assert_eq!(read_goal(&conn_a, &id), read_goal(&conn_b, &id));
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
}

#[test]
fn goal_replay_guards_park_with_same_coded_error() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    let id = create_savings_goal(&conn_a, &goal_input("买车", 100_000)).unwrap();
    let wire = wire_out(&conn_a);
    let bad_create: serde_json::Value = serde_json::from_str(&wire[1]).unwrap();
    let bad_create = serde_json::to_string(&{
        let mut v = bad_create;
        v["command"]["payload"]["target_amount_cents"] = serde_json::json!(-5);
        v
    })
    .unwrap();
    let reports = wire_in(&conn_b, &[wire[0].clone(), bad_create]);
    assert_eq!(reports[0].outcome, OpOutcome::Applied, "夹具账户 op 不受阻");
    assert!(
        matches!(
            &reports[1].outcome,
            OpOutcome::Parked { code, .. } if code == "savings-goal.target-amount-positive"
        ),
        "重放侧金额守卫与本地同码：{reports:?}"
    );
    assert!(read_goal(&conn_b, &id).is_none(), "非正金额创建不落地");
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1);

    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(
        reports
            .iter()
            .all(|r| matches!(r.outcome, OpOutcome::Applied | OpOutcome::Skipped)),
    );
    assert!(read_goal(&conn_b, &id).is_some());
    assert!(parked_ops(&conn_b).unwrap().is_empty());

    update_savings_goal(
        &conn_a,
        &id,
        &SavingsGoalUpdateInput {
            name: "买车基金".into(),
            target_amount_cents: 120_000,
            deadline: None,
            planned_monthly_cents: None,
        },
    )
    .unwrap();
    let wire = wire_out(&conn_a);
    let update_op = wire.last().unwrap().clone();
    let tamper = |field: &str, value: serde_json::Value| {
        let mut v: serde_json::Value = serde_json::from_str(&update_op).unwrap();
        v["command"]["payload"][field] = value;
        serde_json::to_string(&v).unwrap()
    };

    let reports = wire_in(&conn_b, &[tamper("name", serde_json::json!(""))]);
    assert!(
        matches!(
            &reports[0].outcome,
            OpOutcome::Parked { code, .. } if code == "savings-goal.name-required"
        ),
        "重放侧名称守卫与本地同码：{reports:?}"
    );
    let reports = wire_in(
        &conn_b,
        &[tamper("target_amount_cents", serde_json::json!(-5))],
    );
    assert!(
        matches!(
            &reports[0].outcome,
            OpOutcome::Parked { code, .. } if code == "savings-goal.target-amount-positive"
        ),
        "编辑侧金额守卫与本地同码（重复失败投递按 op 幂等覆盖挂起行）：{reports:?}"
    );
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1, "重复投递不堆积");
    assert_eq!(
        read_goal(&conn_b, &id).unwrap(),
        ("买车".into(), 100_000, None, None, "active".into(), 0),
        "守卫挂起不落地，目标行保持合法旧值"
    );

    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(
        reports
            .iter()
            .all(|r| matches!(r.outcome, OpOutcome::Applied | OpOutcome::Skipped)),
        "合法编辑 op 落地：{reports:?}"
    );
    assert!(parked_ops(&conn_b).unwrap().is_empty(), "成功即出队");
    assert_eq!(read_goal(&conn_a, &id), read_goal(&conn_b, &id));
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
}

#[test]
fn goal_schema_ahead_op_parks_and_self_heals_after_upgrade() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    let id = create_savings_goal(&conn_a, &goal_input("买车", 100_000)).unwrap();
    let wire = wire_out(&conn_a);
    let mut op: serde_json::Value = serde_json::from_str(&wire[1]).unwrap();
    op["schema_version"] = serde_json::json!(op["schema_version"].as_i64().unwrap() + 1);
    let ahead = serde_json::to_string(&op).unwrap();

    let reports = wire_in(&conn_b, &[wire[0].clone(), ahead]);
    assert_eq!(reports[0].outcome, OpOutcome::Applied);
    assert!(
        matches!(
            &reports[1].outcome,
            OpOutcome::Parked { code, .. } if code == "sync-engine.schema-ahead"
        ),
        "schema 超前挂起并携带码化原因：{reports:?}"
    );
    assert!(read_goal(&conn_b, &id).is_none(), "未执行不落地");
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 1);

    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert!(
        reports
            .iter()
            .all(|r| matches!(r.outcome, OpOutcome::Applied | OpOutcome::Skipped)),
        "升级后目标 op 落地：{reports:?}"
    );
    assert!(parked_ops(&conn_b).unwrap().is_empty(), "成功即出队");
    assert!(read_goal(&conn_b, &id).is_some(), "目标行落地");
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
}

#[test]
fn goal_payload_undecodable_parks_and_never_drops() {
    let conn = test_support::open();
    seed_device(&conn, "dev-b");

    let unknown_action = r#"{"op_id":"op-goal-future","device_id":"dev-x","clock":9,"schema_version":20,"command":{"entity":"goal","payload":{"action":"future_action","id":"g-1"}}}"#;
    let reports = ingest_ops(&conn, &[unknown_action.into()]).unwrap();
    assert!(
        matches!(
            &reports[0].outcome,
            OpOutcome::Parked { code, .. } if code == "sync-engine.op-undecodable"
        ),
        "不可解码挂起并携带码化原因：{reports:?}"
    );
    let parked = parked_ops(&conn).unwrap();
    assert_eq!(parked.len(), 1, "全部入队，零静默丢弃");
    assert_eq!(parked[0].op_id, "op-goal-future", "信封可读则身份保留");
    assert_eq!(parked[0].entity, "goal");
    assert_eq!(parked[0].payload, unknown_action, "载荷原样保存供裁决");
    assert!(read_ops(&conn).unwrap().is_empty(), "挂起不进日志");
}
