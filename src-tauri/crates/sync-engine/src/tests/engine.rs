use super::super::{
    ApplyReport, DomainCommand, OpOutcome, apply_ops, parked_ops, read_ops, stream_positions,
};
use super::common::{make_expense, read_transaction, wire_in, wire_out};
use ledger_transaction::write::protocol;
use rusqlite::Connection;
use tauri_app_lib::test_support::{
    self, assert_balance_cache_matches_realtime, seed_account, seed_fx_rate_history,
};

struct Ends {
    conn_a: Connection,
    conn_b: Connection,
    _dirs: Vec<test_support::ScratchDir>,
}

impl Ends {
    fn memory() -> Self {
        Self {
            conn_a: test_support::open(),
            conn_b: test_support::open(),
            _dirs: Vec::new(),
        }
    }

    fn file() -> Self {
        let (conn_a, dir_a) = test_support::open_file_scratch("sync-converge-a");
        let (conn_b, dir_b) = test_support::open_file_scratch("sync-converge-b");
        Self {
            conn_a,
            conn_b,
            _dirs: vec![dir_a, dir_b],
        }
    }
}

#[test]
fn a_writes_b_replays_ledger_converges() {
    for ends in [Ends::memory(), Ends::file()] {
        seed_account(&ends.conn_a, "acc-1", "现金", "cash", "CNY", 0);
        seed_account(&ends.conn_b, "acc-1", "现金", "cash", "CNY", 0);

        let created = protocol::create(&ends.conn_a, make_expense("acc-1", 10000, "午饭")).unwrap();

        let ops = read_ops(&ends.conn_a).unwrap();
        assert_eq!(ops.len(), 1);

        let reports = apply_ops(&ends.conn_b, &ops).unwrap();
        assert_eq!(
            reports,
            vec![ApplyReport {
                op_id: ops[0].op_id.clone(),
                outcome: OpOutcome::Applied,
            }]
        );

        let expected = read_transaction(&ends.conn_a, &created.id).unwrap();
        assert_eq!(
            read_transaction(&ends.conn_b, &created.id).unwrap(),
            expected
        );
        assert_eq!(expected.amount_native_cents, 10000);
        assert_eq!(expected.is_deleted, 0);
        assert_balance_cache_matches_realtime(&ends.conn_b);
    }
}

#[test]
fn fx_rate_trace_rides_op_and_converges() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-hkd", "港币户", "cash", "HKD", 0);
    seed_account(&conn_b, "acc-hkd", "港币户", "cash", "HKD", 0);
    seed_fx_rate_history(&conn_a, "fxh-sync", "HKD", "CNY", "2026-01-05", 0.9);

    let mut input = make_expense("acc-hkd", 10000, "港币午饭");
    input.currency_code = "HKD".into();
    input.date = "2026-01-07".into();
    let created = protocol::create(&conn_a, input).unwrap();

    apply_ops(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();

    let expected = read_transaction(&conn_a, &created.id).unwrap();
    assert_eq!(expected.amount_native_cents, 9000);
    assert_eq!(expected.fx_rate_used, Some(0.9));
    assert_eq!(
        read_transaction(&conn_b, &created.id).unwrap(),
        expected,
        "重放端应收敛到与源端一致的行（含折算留痕）"
    );
    assert_balance_cache_matches_realtime(&conn_b);
}

#[test]
fn legacy_op_without_fx_trace_fields_replays_with_null_trace() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-hkd", "港币户", "cash", "HKD", 0);
    seed_account(&conn_b, "acc-hkd", "港币户", "cash", "HKD", 0);
    seed_fx_rate_history(&conn_a, "fxh-sync", "HKD", "CNY", "2026-01-05", 0.9);

    let mut input = make_expense("acc-hkd", 10000, "港币午饭");
    input.currency_code = "HKD".into();
    input.date = "2026-01-07".into();
    let created = protocol::create(&conn_a, input).unwrap();

    let mut legacy: serde_json::Value = serde_json::from_str(&wire_out(&conn_a)[0]).unwrap();
    legacy["schema_version"] = serde_json::json!(22);
    let row = legacy["command"]["payload"]["row"].as_object_mut().unwrap();
    let removed = row.remove("fx_rate_used").is_some() && row.remove("fx_rate_source").is_some();
    assert!(removed, "前置：op 行载荷应携带留痕字段");
    let raw = serde_json::to_string(&legacy).unwrap();

    let reports = wire_in(&conn_b, &[raw]);
    assert!(
        matches!(reports[0].outcome, OpOutcome::Applied),
        "旧格式 op 应照常应用，实际: {:?}",
        reports[0].outcome
    );
    let row = read_transaction(&conn_b, &created.id).unwrap();
    assert_eq!(row.amount_native_cents, 9000, "源端折算结果照常搬运");
    assert_eq!(row.fx_rate_used, None, "旧载荷缺省：无汇率留痕");
    assert_eq!(row.fx_rate_source, None, "旧载荷缺省：无来源留痕");
    assert!(parked_ops(&conn_b).unwrap().is_empty());
}

#[test]
fn redelivery_of_same_ops_is_idempotent() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    protocol::create(&conn_a, make_expense("acc-1", 10000, "午饭")).unwrap();
    let ops = read_ops(&conn_a).unwrap();
    apply_ops(&conn_b, &ops).unwrap();

    let reports = apply_ops(&conn_b, &ops).unwrap();
    assert!(reports.iter().all(|r| r.outcome == OpOutcome::Skipped));

    let count: i64 = conn_b
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "重复投递不产生第二笔交易");

    protocol::create(&conn_a, make_expense("acc-1", 500, "咖啡")).unwrap();
    let all_a = read_ops(&conn_a).unwrap();
    assert_eq!(all_a.len(), 2);
    let mut mixed = ops;
    mixed.push(all_a[1].clone());
    let reports = apply_ops(&conn_b, &mixed).unwrap();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].outcome, OpOutcome::Skipped, "旧 op 已知，跳过");
    assert_eq!(reports[1].outcome, OpOutcome::Applied, "新 op 执行");
}

#[test]
fn update_and_delete_replay_keep_both_sides_equal() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    let id = protocol::create(&conn_a, make_expense("acc-1", 10000, "午饭"))
        .unwrap()
        .id;
    apply_ops(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();

    protocol::update(&conn_a, &id, make_expense("acc-1", 25000, "午饭（改）")).unwrap();
    apply_ops(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();
    let expected = read_transaction(&conn_a, &id).unwrap();
    assert_eq!(read_transaction(&conn_b, &id).unwrap(), expected);
    assert_eq!(expected.amount_native_cents, 25000);

    protocol::delete(&conn_a, &id).unwrap();
    apply_ops(&conn_b, &read_ops(&conn_a).unwrap()).unwrap();
    let expected = read_transaction(&conn_a, &id).unwrap();
    assert_eq!(read_transaction(&conn_b, &id).unwrap(), expected);
    assert_eq!(expected.is_deleted, 1);
    assert_balance_cache_matches_realtime(&conn_b);
}

#[test]
fn cross_device_ops_converge_to_same_total_order() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    protocol::create(&conn_a, make_expense("acc-1", 10000, "A 记")).unwrap();
    protocol::create(&conn_b, make_expense("acc-1", 20000, "B 记")).unwrap();

    let ops_a = read_ops(&conn_a).unwrap();
    let ops_b = read_ops(&conn_b).unwrap();
    apply_ops(&conn_b, &ops_a).unwrap();
    apply_ops(&conn_a, &ops_b).unwrap();

    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
    let both = read_ops(&conn_a).unwrap();
    assert_eq!(both.len(), 2);
    assert_eq!(both[0].clock, both[1].clock, "两端并发：端内时钟同为 1");
    assert_ne!(both[0].device_id, both[1].device_id);
    assert!(
        both[0].device_id < both[1].device_id,
        "同钟并列按 DeviceId 字典序 tiebreak"
    );
    for op in &both {
        let DomainCommand::Transaction(ledger_transaction::TransactionCommand::Create {
            id, ..
        }) = &op.command
        else {
            panic!("应为交易 create 命令");
        };
        assert_eq!(
            read_transaction(&conn_a, id),
            read_transaction(&conn_b, id),
            "两端对同一笔交易的账本状态一致"
        );
    }
    assert_balance_cache_matches_realtime(&conn_a);
    assert_balance_cache_matches_realtime(&conn_b);
}

#[test]
fn apply_ops_on_empty_batch_is_noop() {
    let conn = test_support::open();
    let reports = apply_ops(&conn, &[]).unwrap();
    assert!(reports.is_empty());
    assert!(read_ops(&conn).unwrap().is_empty());
}

#[test]
fn dependency_failure_parks_without_blocking_batch_and_redelivery_applies() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    seed_account(&conn_a, "acc-a-only", "A 独有账户", "cash", "CNY", 0);
    protocol::create(&conn_a, make_expense("acc-1", 10000, "好的")).unwrap();
    let bad = protocol::create(&conn_a, make_expense("acc-a-only", 500, "坏引用"))
        .unwrap()
        .id;
    let ops = read_ops(&conn_a).unwrap();
    assert_eq!(ops.len(), 2);

    let reports = apply_ops(&conn_b, &ops).unwrap();
    assert!(
        matches!(&reports[0].outcome, OpOutcome::Applied),
        "好 op 不受阻塞"
    );
    assert!(
        matches!(&reports[1].outcome, OpOutcome::Parked { code, .. } if !code.is_empty()),
        "坏引用挂起并发码化错误"
    );
    assert!(
        !read_ops(&conn_b)
            .unwrap()
            .iter()
            .any(|op| op.op_id == ops[1].op_id)
    );
    assert!(read_transaction(&conn_b, &bad).is_none());
    let parked = parked_ops(&conn_b).unwrap();
    assert_eq!(parked.len(), 1);
    assert_eq!(parked[0].op_id, ops[1].op_id);
    assert_eq!(parked[0].entity, "transaction");

    seed_account(&conn_b, "acc-a-only", "A 独有账户", "cash", "CNY", 0);
    let reports = apply_ops(&conn_b, &ops).unwrap();
    assert_eq!(reports[0].outcome, OpOutcome::Skipped, "已应用 op 幂等跳过");
    assert_eq!(reports[1].outcome, OpOutcome::Applied, "重投递自然重试成功");
    assert!(
        read_transaction(&conn_b, &bad).is_some(),
        "补齐后重投递落地"
    );
    assert!(parked_ops(&conn_b).unwrap().is_empty(), "成功即出队");
    assert_balance_cache_matches_realtime(&conn_b);
}

#[test]
fn replay_transaction_is_atomic_across_both_dbs() {
    let ends = Ends::file();
    let conn_a = &ends.conn_a;
    let conn_b = &ends.conn_b;
    seed_account(conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(conn_b, "acc-1", "现金", "cash", "CNY", 0);

    protocol::create(conn_a, make_expense("acc-1", 10000, "午饭")).unwrap();
    let ops = read_ops(conn_a).unwrap();
    assert_eq!(ops.len(), 1);

    test_support::block_op_writes(conn_b);
    let reports = apply_ops(conn_b, &ops).unwrap();
    assert!(
        matches!(&reports[0].outcome, OpOutcome::Parked { .. }),
        "重放失败按既有接缝挂起，实际: {:?}",
        reports[0].outcome
    );

    let txn_rows: i64 = conn_b
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(txn_rows, 0, "业务行不残留（main）");
    let op_rows: i64 = conn_b
        .query_row("SELECT COUNT(*) FROM sync.sync_ops", [], |r| r.get(0))
        .unwrap();
    assert_eq!(op_rows, 0, "外来 op 不落日志（sync）");
    assert!(
        stream_positions(conn_b)
            .unwrap()
            .iter()
            .all(|p| p.device_id != ops[0].device_id),
        "位点不推进（sync）"
    );

    test_support::unblock_op_writes(conn_b);
    let reports = apply_ops(conn_b, &ops).unwrap();
    assert_eq!(reports[0].outcome, OpOutcome::Applied, "重投递自然重试成功");
    let txn_rows: i64 = conn_b
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(txn_rows, 1, "业务行落 main");
    let op_rows: i64 = conn_b
        .query_row("SELECT COUNT(*) FROM sync.sync_ops", [], |r| r.get(0))
        .unwrap();
    assert_eq!(op_rows, 1, "外来 op 落日志（sync）");
    let positions = stream_positions(conn_b).unwrap();
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].device_id, ops[0].device_id);
    assert_eq!(positions[0].applied_through, ops[0].clock, "位点推进到位");
    assert!(parked_ops(conn_b).unwrap().is_empty(), "成功即出队");
    assert_balance_cache_matches_realtime(conn_b);
}
