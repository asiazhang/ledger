use super::super::{OpOutcome, ingest_ops, parked_ops, read_ops};
use super::common::{make_expense, read_transaction, wire_in, wire_out};
use ledger_accounts::delete_account;
use ledger_transaction::write::protocol;
use tauri_app_lib::test_support::{self, seed_account};

#[test]
fn schema_ahead_op_parks_with_upgrade_hint() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    let created = protocol::create(&conn_a, make_expense("acc-1", 10000, "午饭"))
        .unwrap()
        .id;
    let mut op = read_ops(&conn_a).unwrap().remove(0);
    op.schema_version += 1;
    let ahead = serde_json::to_string(&op).unwrap();

    protocol::create(&conn_a, make_expense("acc-1", 500, "咖啡")).unwrap();
    let good_op = wire_out(&conn_a)[1].clone();
    let reports = ingest_ops(&conn_b, &[good_op, ahead]).unwrap();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].outcome, OpOutcome::Applied, "正常 op 不受阻");
    assert!(
        matches!(
            &reports[1].outcome,
            OpOutcome::Parked { code, .. } if code == "sync-engine.schema-ahead"
        ),
        "schema 超前挂起并携带码化原因"
    );

    let parked = parked_ops(&conn_b).unwrap();
    assert_eq!(parked.len(), 1);
    assert_eq!(parked[0].op_id, op.op_id);
    assert_eq!(parked[0].schema_version, op.schema_version);
    assert!(
        read_transaction(&conn_b, &created).is_none(),
        "未执行不落地"
    );
}

#[test]
fn undecodable_wire_ops_park_and_never_drop() {
    let conn = test_support::open();
    seed_account(&conn, "acc-1", "现金", "cash", "CNY", 0);

    let unknown_entity = r#"{"op_id":"op-future","device_id":"dev-x","clock":9,"schema_version":20,"command":{"entity":"future-entity","payload":{"anything":1}}}"#;
    let garbage = "not-json-at-all";

    let reports = ingest_ops(&conn, &[unknown_entity.into(), garbage.into()]).unwrap();
    assert_eq!(reports.len(), 2);
    for report in &reports {
        assert!(
            matches!(
                &report.outcome,
                OpOutcome::Parked { code, .. } if code == "sync-engine.op-undecodable"
            ),
            "不可解码挂起并携带码化原因"
        );
    }

    let parked = parked_ops(&conn).unwrap();
    assert_eq!(parked.len(), 2, "全部入队，零静默丢弃");
    let future = parked
        .iter()
        .find(|p| p.op_id == "op-future")
        .expect("信封可读则身份保留");
    assert_eq!(future.entity, "future-entity");
    assert_eq!(
        future.payload, unknown_entity,
        "载荷原样保存（供升级后重放与裁决）"
    );
    let synthetic = parked
        .iter()
        .find(|p| p.op_id != "op-future")
        .expect("信封损坏另有条目");
    assert!(
        synthetic.op_id.starts_with("parked-"),
        "信封不可读则合成 id"
    );
    assert_eq!(synthetic.payload, garbage);
    assert!(read_ops(&conn).unwrap().is_empty(), "挂起不进日志");
    for row in &parked {
        assert_eq!(row.code, "sync-engine.op-undecodable");
        assert_eq!(row.params.len(), 1, "解析详情应作唯一插值参数");
        assert!(!row.params[0].is_empty());
        assert!(
            row.message.ends_with(&row.params[0]),
            "message 已渲染：应以 params[0] 结尾，实际 {:?}",
            row.message
        );
    }
}

#[test]
fn replay_onto_deleted_account_parks_and_never_resurrects() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_a, "acc-x", "将删账户", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-x", "将删账户", "cash", "CNY", 0);

    let id = protocol::create(&conn_a, make_expense("acc-x", 10000, "午饭"))
        .unwrap()
        .id;
    wire_in(&conn_b, &wire_out(&conn_a));

    delete_account(&conn_b, "acc-x").unwrap();

    protocol::update(&conn_a, &id, make_expense("acc-x", 10000, "午饭（改）")).unwrap();
    protocol::create(&conn_a, make_expense("acc-x", 500, "咖啡")).unwrap();
    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert_eq!(reports[0].outcome, OpOutcome::Skipped, "已应用 op 幂等跳过");
    for report in &reports[1..] {
        assert!(
            matches!(&report.outcome,
                OpOutcome::Parked { code, .. } if code == "account.not-found"),
            "往已删账户记账挂起并发码化错误"
        );
    }

    assert_eq!(
        read_transaction(&conn_b, &id).unwrap().note.as_deref(),
        Some("午饭"),
        "修改未落地"
    );
    let count: i64 = conn_b
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "新创建未落地");
    let alive: i64 = conn_b
        .query_row(
            "SELECT COUNT(*) FROM accounts WHERE id='acc-x' AND is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(alive, 0, "已删账户不复活");
    let parked = parked_ops(&conn_b).unwrap();
    assert_eq!(parked.len(), 2);
    for row in &parked {
        assert_eq!(row.code, "account.not-found");
        assert_eq!(
            row.params,
            vec!["acc-x".to_string()],
            "params 应携带账户 id"
        );
        assert!(
            row.message.contains("acc-x"),
            "message 是已渲染完整句，应含动态值，实际 {:?}",
            row.message
        );
    }

    wire_in(&conn_b, &wire_out(&conn_a));
    assert_eq!(parked_ops(&conn_b).unwrap().len(), 2, "重投递不堆积挂起行");
    let count: i64 = conn_b
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "仍不落地");
}

#[test]
fn generic_kind_currency_divergence_parks_without_booking() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    protocol::create(&conn_a, make_expense("acc-1", 10000, "午饭")).unwrap();
    let bad = protocol::create(&conn_a, make_expense("acc-1", 500, "咖啡"))
        .unwrap()
        .id;
    let wire = wire_out(&conn_a);
    let mut tampered: serde_json::Value = serde_json::from_str(&wire[1]).unwrap();
    tampered["command"]["payload"]["row"]["currency_code"] = serde_json::json!("USD");
    let reports = wire_in(
        &conn_b,
        &[wire[0].clone(), serde_json::to_string(&tampered).unwrap()],
    );
    assert_eq!(reports[0].outcome, OpOutcome::Applied, "批内其余 op 不受阻");
    assert!(
        matches!(&reports[1].outcome, OpOutcome::Parked { code, .. } if code == "transaction.currency-mismatch"),
        "币种不一致应码化挂起，实际: {:?}",
        reports[1]
    );
    assert_eq!(
        read_ops(&conn_b).unwrap().len(),
        1,
        "只有正常 op 落日志，挂起者不落"
    );
    assert!(read_transaction(&conn_b, &bad).is_none(), "不落错币行");
    let parked = parked_ops(&conn_b).unwrap();
    assert_eq!(parked.len(), 1);
    assert_eq!(parked[0].params, vec!["CNY".to_string(), "USD".to_string()]);
}
