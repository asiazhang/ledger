use super::super::{ApplyReport, DomainCommand, OpOutcome, parked_ops, read_ops};
use super::common::{
    make_expense, read_occurrence, read_transaction, seed_device, seed_occurrence, seed_plan,
    wire_in, wire_out,
};
use ledger_investment::{MarketPriceInput, create_market_price};
use ledger_scheduled::{execute_occurrence, occurrence_transaction_id};
use ledger_transaction::write::protocol;
use tauri_app_lib::test_support::{
    self, assert_balance_cache_matches_realtime, seed_account, seed_instrument,
};

#[test]
fn concurrent_same_field_edits_converge_to_order_last() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    let id = protocol::create(&conn_a, make_expense("acc-1", 10000, "午饭"))
        .unwrap()
        .id;
    wire_in(&conn_b, &wire_out(&conn_a));

    protocol::update(&conn_a, &id, make_expense("acc-1", 10000, "A 改")).unwrap();
    protocol::update(&conn_b, &id, make_expense("acc-1", 10000, "B 改")).unwrap();

    let reports_b = wire_in(&conn_b, &wire_out(&conn_a));
    let reports_a = wire_in(&conn_a, &wire_out(&conn_b));

    let expected = read_transaction(&conn_a, &id).unwrap();
    assert_eq!(expected.note.as_deref(), Some("A 改"), "全序末者胜");
    assert_eq!(read_transaction(&conn_b, &id).unwrap(), expected);

    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
    assert!(
        read_ops(&conn_a).unwrap().iter().any(|op| matches!(
            &op.command,
            DomainCommand::Transaction(ledger_transaction::TransactionCommand::Update { row, .. })
                if row.note.as_deref() == Some("B 改")
        )),
        "输者的 op 仍可审计"
    );

    assert_eq!(
        outcomes(&reports_a),
        vec![
            OpOutcome::Skipped,
            OpOutcome::Superseded,
            OpOutcome::Skipped
        ],
        "A 端：A 的旧 op 跳过，B 的并发修改被 LWW 压制"
    );
    assert_eq!(
        outcomes(&reports_b),
        vec![OpOutcome::Skipped, OpOutcome::Applied],
        "B 端：旧 op 跳过，序末者执行"
    );
    assert_balance_cache_matches_realtime(&conn_a);
    assert_balance_cache_matches_realtime(&conn_b);
}

#[test]
fn concurrent_market_price_edits_converge_to_order_last() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    seed_instrument(&conn_a, "inst-1", "SYM", "Symbol", "USD", "unknown");
    seed_instrument(&conn_b, "inst-1", "SYM", "Symbol", "USD", "unknown");

    let write = |conn: &rusqlite::Connection, price_cents: i64| {
        create_market_price(
            conn,
            MarketPriceInput {
                instrument_id: "inst-1".into(),
                price_cents,
                currency_code: "USD".into(),
                priced_at: "2026-01-10".into(),
                source: None,
            },
        )
        .unwrap();
    };
    write(&conn_a, 200);
    write(&conn_b, 100);

    let reports_b = wire_in(&conn_b, &wire_out(&conn_a));
    let reports_a = wire_in(&conn_a, &wire_out(&conn_b));

    assert_eq!(
        outcomes(&reports_b),
        vec![OpOutcome::Superseded],
        "B 端：A 的同实体早序 op 被 LWW 压制"
    );
    assert_eq!(
        outcomes(&reports_a),
        vec![OpOutcome::Skipped, OpOutcome::Applied],
        "A 端：回传日志含本端已知 op（跳过），B 的序末 op 执行"
    );

    let current = |conn: &rusqlite::Connection| -> i64 {
        conn.query_row(
            "SELECT price_cents FROM market_prices WHERE instrument_id='inst-1'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(current(&conn_a), 100, "A 端收敛到全序赢者");
    assert_eq!(current(&conn_b), 100, "B 端收敛到全序赢者");
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
}

#[test]
fn delete_vs_concurrent_update_never_resurrects_loser_side_parks() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    seed_account(&conn_a, "acc-1", "现金", "cash", "CNY", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);

    let id = protocol::create(&conn_a, make_expense("acc-1", 10000, "午饭"))
        .unwrap()
        .id;
    wire_in(&conn_b, &wire_out(&conn_a));

    protocol::update(&conn_a, &id, make_expense("acc-1", 10000, "A 改")).unwrap();
    protocol::delete(&conn_b, &id).unwrap();

    wire_in(&conn_b, &wire_out(&conn_a));
    wire_in(&conn_a, &wire_out(&conn_b));

    assert_eq!(
        read_transaction(&conn_a, &id).unwrap().note.as_deref(),
        Some("A 改")
    );
    assert_eq!(read_transaction(&conn_b, &id).unwrap().is_deleted, 1);
    let parked = parked_ops(&conn_b).unwrap();
    assert_eq!(parked.len(), 1);
    assert_eq!(parked[0].code, "transaction.not-found");
}

fn outcomes(reports: &[ApplyReport]) -> Vec<OpOutcome> {
    reports.iter().map(|r| r.outcome.clone()).collect()
}

const PLAN: &str = "plan-1";
const FEB: &str = "2026-02-01";

#[test]
fn concurrent_occurrence_execution_lands_exactly_once() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    for conn in [&conn_a, &conn_b] {
        seed_account(conn, "acc-1", "现金", "cash", "CNY", 0);
        seed_plan(conn, PLAN, "acc-1", 3000);
    }
    seed_occurrence(&conn_a, "occ-a", PLAN, FEB, 3000);
    seed_occurrence(&conn_b, "occ-b", PLAN, FEB, 3000);

    let landed_a = execute_occurrence(&conn_a, "occ-a").unwrap();
    let landed_b = execute_occurrence(&conn_b, "occ-b").unwrap();
    assert_eq!(
        landed_a, landed_b,
        "同一期次的落地身份跨端一致（OccurrenceKey 派生）"
    );

    let reports_b = wire_in(&conn_b, &wire_out(&conn_a));
    let reports_a = wire_in(&conn_a, &wire_out(&conn_b));
    assert!(
        reports_a
            .iter()
            .chain(&reports_b)
            .any(|r| r.outcome == OpOutcome::Deduped),
        "对端期次 op 按键去重"
    );

    for (conn, occ) in [(&conn_a, "occ-a"), (&conn_b, "occ-b")] {
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "该期次只落一次");
        assert_eq!(
            read_transaction(conn, &landed_a).unwrap().amount_cents,
            3000
        );
        assert_eq!(
            read_occurrence(conn, occ).unwrap(),
            ("completed".into(), Some(landed_a.clone())),
            "期次完成并回填落地交易"
        );
    }
    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
    assert_balance_cache_matches_realtime(&conn_a);
    assert_balance_cache_matches_realtime(&conn_b);
}

#[test]
fn single_side_execution_replays_and_completes_peer_occurrence() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    for conn in [&conn_a, &conn_b] {
        seed_account(conn, "acc-1", "现金", "cash", "CNY", 0);
        seed_plan(conn, PLAN, "acc-1", 3000);
    }
    seed_occurrence(&conn_a, "occ-a", PLAN, FEB, 3000);
    seed_occurrence(&conn_b, "occ-b", PLAN, FEB, 3000);

    let landed = execute_occurrence(&conn_a, "occ-a").unwrap();
    let reports = wire_in(&conn_b, &wire_out(&conn_a));
    assert_eq!(outcomes(&reports), vec![OpOutcome::Applied]);

    assert_eq!(
        read_transaction(&conn_b, &landed).unwrap().amount_cents,
        3000
    );
    assert_eq!(
        read_occurrence(&conn_b, "occ-b").unwrap(),
        ("completed".into(), Some(landed.clone()))
    );
    assert_balance_cache_matches_realtime(&conn_b);

    assert!(execute_occurrence(&conn_b, "occ-b").is_err());
    let count: i64 = conn_b
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn execution_with_already_landed_transaction_completes_without_second_row() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    for conn in [&conn_a, &conn_b] {
        seed_account(conn, "acc-1", "现金", "cash", "CNY", 0);
        seed_plan(conn, PLAN, "acc-1", 3000);
    }
    seed_occurrence(&conn_a, "occ-a", PLAN, FEB, 3000);

    let landing = execute_occurrence(&conn_a, "occ-a").unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));
    assert!(read_occurrence(&conn_b, "occ-late").is_none());

    seed_occurrence(&conn_b, "occ-late", PLAN, FEB, 3000);
    let again = execute_occurrence(&conn_b, "occ-late").unwrap();
    assert_eq!(again, landing, "落地身份一致，不插第二笔");
    let count: i64 = conn_b
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "同一期次只有一笔交易");
    assert_eq!(
        read_occurrence(&conn_b, "occ-late").unwrap(),
        ("completed".into(), Some(landing)),
        "期次完成并回填既有落地"
    );
    assert_balance_cache_matches_realtime(&conn_b);
}

#[test]
fn occurrence_command_has_no_entity_subject() {
    let conn = test_support::open();
    seed_account(&conn, "acc-1", "现金", "cash", "CNY", 0);
    seed_plan(&conn, PLAN, "acc-1", 3000);
    seed_occurrence(&conn, "occ-a", PLAN, FEB, 3000);
    execute_occurrence(&conn, "occ-a").unwrap();
    let ops = read_ops(&conn).unwrap();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].command.subject().0, "scheduled");
    assert!(
        ops[0].command.subject().1.is_none(),
        "期次触发无实体键（冲突域在 OccurrenceKey）"
    );
    let DomainCommand::Scheduled(cmd) = &ops[0].command else {
        panic!("应为期次触发命令");
    };
    let (plan_id, scheduled_date) = match cmd {
        ledger_scheduled::ScheduledCommand::ExecuteOccurrence {
            plan_id,
            scheduled_date,
            ..
        } => (plan_id, scheduled_date),
        _ => panic!("应为期次触发命令"),
    };
    assert_eq!(plan_id, PLAN);
    assert_eq!(scheduled_date, FEB);
    assert_eq!(
        occurrence_transaction_id(plan_id, scheduled_date),
        cmd.landing_transaction_id(),
        "落地身份由 OccurrenceKey 派生"
    );
}
