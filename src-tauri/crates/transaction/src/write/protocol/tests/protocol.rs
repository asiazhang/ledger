use rusqlite::Connection;

use crate::tests::common::make_input;
use ledger_infra::error::AppError;
use ledger_sync_engine::read_ops;
use tauri_app_lib::ledger_transaction::TransactionCommand;
use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::ledger_transaction::*;
use tauri_app_lib::test_support::{self, seed_account, seed_instrument};

fn carried_row(kind: TransactionKind, account: &str, amount: i64) -> NormalizedTransaction {
    NormalizedTransaction {
        funding: Vec::new(),
        kind,
        amount_cents: amount,
        currency_code: "CNY".into(),
        amount_native_cents: amount,
        fx_rate_used: None,
        fx_rate_source: None,
        account_id: Some(account.into()),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        policy_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
        note: None,
        date: "2026-05-04".into(),
        purchases: Vec::new(),
    }
}

fn coded_of(err: AppError) -> (String, String) {
    match err {
        AppError::Coded { code, message, .. } => (code, message),
        other => panic!("应为码化错误，实际: {other:?}"),
    }
}

fn seed_buy_and_convert(conn: &Connection) -> (String, String) {
    seed_account(conn, "acc-cv", "基金户", "investment", "CNY", 0);
    seed_instrument(conn, "inst-out", "006793", "转出基金", "CNY", "unknown");
    seed_instrument(conn, "inst-in", "519700", "转入基金", "CNY", "unknown");
    let buy_input = tauri_app_lib::ledger_transaction::TransactionInput {
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: Some("acc-cv".into()),
        date: "2026-01-10".into(),
        instrument_id: Some("inst-out".into()),
        quantity: Some(10.0),
        price_cents: Some(10_000),
        fee_cents: Some(0),
        ..make_input("acc-cv", TransactionKind::Buy, 0, "2026-01-10")
    };
    let buy_id = create_transaction_internal(conn, buy_input).unwrap().id;
    let convert_input = tauri_app_lib::ledger_transaction::TransactionInput {
        kind: TransactionKind::Convert,
        amount_cents: 0,
        account_id: Some("acc-cv".into()),
        date: "2026-02-01".into(),
        instrument_id: Some("inst-out".into()),
        quantity: Some(10.0),
        to_instrument_id: Some("inst-in".into()),
        to_quantity: Some(10.0),
        out_amount_cents: Some(1_100),
        in_amount_cents: Some(1_100),
        fee_cents: Some(0),
        ..make_input("acc-cv", TransactionKind::Convert, 0, "2026-02-01")
    };
    let convert_id = create_transaction_internal(conn, convert_input).unwrap().id;
    (buy_id, convert_id)
}

#[test]
fn create_dividend_same_terminal_state_both_forms() {
    let seed = |conn: &Connection| {
        seed_account(conn, "acc-div", "股票户", "investment", "CNY", 0);
        seed_instrument(conn, "inst-div", "502010", "证券基金", "CNY", "unknown");
    };

    let conn_local = test_support::open();
    seed(&conn_local);
    let local_id = create_transaction_internal(
        &conn_local,
        tauri_app_lib::ledger_transaction::TransactionInput {
            instrument_id: Some("inst-div".into()),
            ..make_input("acc-div", TransactionKind::Dividend, 3000, "2026-05-04")
        },
    )
    .unwrap()
    .id;

    let conn_replay = test_support::open();
    seed(&conn_replay);
    replay_command(
        &conn_replay,
        &TransactionCommand::Create {
            id: "sync-div-1".into(),
            row: carried_row(TransactionKind::Dividend, "acc-div", 3000),
            investment: Some(tauri_app_lib::ledger_transaction::InvestmentCommandFields {
                instrument_id: "inst-div".into(),
                quantity: 0.0,
                price_cents: 0,
                fee_cents: 0,
                cost_per_unit_cents: None,
                origin: None,
            }),
            split: None,
            convert: None,
        },
    )
    .unwrap();

    for (conn, id) in [
        (&conn_local, local_id.as_str()),
        (&conn_replay, "sync-div-1"),
    ] {
        let t = get_transaction_internal(conn, id).unwrap();
        assert_eq!(t.kind, TransactionKind::Dividend);
        assert_eq!(t.amount_cents, 3000);
        assert_eq!(t.currency_code, "CNY");
        let ext: (String, String, Option<f64>, Option<i64>) = conn
            .query_row(
                "SELECT instrument_id, action, quantity, price_cents FROM security_transactions \
                 WHERE transaction_id=?1",
                rusqlite::params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            ext,
            ("inst-div".to_string(), "dividend".to_string(), None, None)
        );
    }
    assert!(read_ops(&conn_replay).unwrap().is_empty());
}

#[test]
fn update_rejects_dividend_split_same_code_both_forms() {
    let expected_code = |kind: TransactionKind| match kind {
        TransactionKind::Split => "trade.split-kind-change-forbidden",
        TransactionKind::Dividend => "trade.dividend-kind-change-forbidden",
        _ => unreachable!("本测试只覆盖 dividend / split"),
    };
    for kind in [TransactionKind::Dividend, TransactionKind::Split] {
        let conn_local = test_support::open();
        seed_account(&conn_local, "acc-g", "现金", "cash", "CNY", 0);
        let id_local = create_transaction_internal(
            &conn_local,
            make_input("acc-g", TransactionKind::Expense, 500, "2026-01-01"),
        )
        .unwrap()
        .id;
        let err_local = update_transaction_internal(
            &conn_local,
            &id_local,
            make_input("acc-g", kind, 500, "2026-05-04"),
        )
        .unwrap_err();

        let conn_replay = test_support::open();
        seed_account(&conn_replay, "acc-g", "现金", "cash", "CNY", 0);
        let id_replay = create_transaction_internal(
            &conn_replay,
            make_input("acc-g", TransactionKind::Expense, 500, "2026-01-01"),
        )
        .unwrap()
        .id;
        let err_replay = replay_command(
            &conn_replay,
            &TransactionCommand::Update {
                id: id_replay.clone(),
                row: carried_row(kind, "acc-g", 500),
                investment: None,
                split: None,
                convert: None,
            },
        )
        .unwrap_err();

        let (code_local, msg_local) = coded_of(err_local);
        let (code_replay, msg_replay) = coded_of(err_replay);
        assert_eq!(code_local, expected_code(kind));
        assert_eq!(code_local, code_replay, "{kind} 两形态应同码");
        assert_eq!(msg_local, msg_replay, "{kind} 两形态应同文案");

        for (conn, id) in [(&conn_local, id_local), (&conn_replay, id_replay)] {
            let t = get_transaction_internal(conn, &id).unwrap();
            assert_eq!(t.kind, TransactionKind::Expense);
            assert_eq!(t.amount_cents, 500);
        }
    }
}

#[test]
fn update_convert_kind_change_same_code_both_forms() {
    let conn_local = test_support::open();
    let (buy_id_l, convert_id_l) = seed_buy_and_convert(&conn_local);
    let err_local_in = update_transaction_internal(
        &conn_local,
        &buy_id_l,
        make_input("acc-cv", TransactionKind::Convert, 0, "2026-03-01"),
    )
    .unwrap_err();
    let err_local_out = update_transaction_internal(
        &conn_local,
        &convert_id_l,
        make_input("acc-cv", TransactionKind::Buy, 0, "2026-03-01"),
    )
    .unwrap_err();

    let conn_replay = test_support::open();
    let (buy_id_r, convert_id_r) = seed_buy_and_convert(&conn_replay);
    let err_replay_in = replay_command(
        &conn_replay,
        &TransactionCommand::Update {
            id: buy_id_r,
            row: carried_row(TransactionKind::Convert, "acc-cv", 1_000),
            investment: None,
            split: None,
            convert: None,
        },
    )
    .unwrap_err();
    let err_replay_out = replay_command(
        &conn_replay,
        &TransactionCommand::Update {
            id: convert_id_r,
            row: carried_row(TransactionKind::Buy, "acc-cv", 0),
            investment: None,
            split: None,
            convert: None,
        },
    )
    .unwrap_err();

    for err in [
        &err_local_in,
        &err_local_out,
        &err_replay_in,
        &err_replay_out,
    ] {
        assert!(
            matches!(err, AppError::Coded { code, .. } if code == "trade.convert-kind-change-forbidden"),
            "改为/改出 convert 应同码拒绝，实际: {err:?}"
        );
    }

    for conn in [&conn_local, &conn_replay] {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2, "守卫拒绝后原行原样在场");
    }
}

#[test]
fn replay_create_uses_carried_id_and_emits_no_local_op() {
    let conn = test_support::open();
    seed_account(&conn, "acc-r", "现金", "cash", "CNY", 0);

    replay_command(
        &conn,
        &TransactionCommand::Create {
            id: "sync-tx-1".into(),
            row: carried_row(TransactionKind::Expense, "acc-r", 6_400),
            investment: None,
            split: None,
            convert: None,
        },
    )
    .unwrap();

    let t = get_transaction_internal(&conn, "sync-tx-1").unwrap();
    assert_eq!(t.kind, TransactionKind::Expense);
    assert_eq!(t.amount_cents, 6_400);
    assert!(
        read_ops(&conn).unwrap().is_empty(),
        "重放形态不得追加本地 op"
    );
}

#[test]
fn create_split_local_and_replay_enter_domain_guards() {
    let conn_local = test_support::open();
    seed_account(&conn_local, "acc-split", "现金", "cash", "CNY", 0);
    let err_local = create_transaction_internal(
        &conn_local,
        make_input("acc-split", TransactionKind::Split, 0, "2026-05-04"),
    )
    .unwrap_err();
    assert_eq!(
        coded_of(err_local).0,
        "trade.split-instrument-required",
        "Local 的 split 应进投资域守卫而非 kind 拒绝"
    );

    let conn_replay = test_support::open();
    seed_account(&conn_replay, "acc-split", "现金", "cash", "CNY", 0);
    let err_replay = replay_command(
        &conn_replay,
        &TransactionCommand::Create {
            id: "sync-split-1".into(),
            row: carried_row(TransactionKind::Split, "acc-split", 0),
            investment: None,
            split: None,
            convert: None,
        },
    )
    .unwrap_err();
    assert_eq!(
        coded_of(err_replay).0,
        "transaction.split-fields-missing",
        "缺份额调整字段的旧 split 载荷应码化挂起（不静默落半套副作用）"
    );

    let err_replay = replay_command(
        &conn_replay,
        &TransactionCommand::Create {
            id: "sync-split-2".into(),
            row: carried_row(TransactionKind::Split, "acc-split", 0),
            investment: None,
            split: Some(
                tauri_app_lib::ledger_transaction::command::SplitCommandFields {
                    instrument_id: "inst-missing".into(),
                    delta_quantity: 5.0,
                    final_quantity: 5.0,
                    total_cost_cents: 0,
                },
            ),
            convert: None,
        },
    )
    .unwrap_err();
    assert_eq!(
        coded_of(err_replay).0,
        "trade.split-instrument-not-found",
        "Replay 的 split 应进投资域重放守卫而非 kind 拒绝"
    );
}

#[test]
fn replay_generic_kind_currency_mismatch_rejects_without_booking() {
    let conn = test_support::open();
    seed_account(&conn, "acc-usd", "美元户", "cash", "USD", 0);
    let err = replay_command(
        &conn,
        &TransactionCommand::Create {
            id: "sync-tx-1".into(),
            row: carried_row(TransactionKind::Income, "acc-usd", 1_000),
            investment: None,
            split: None,
            convert: None,
        },
    )
    .unwrap_err();
    let (code, message) = coded_of(err);
    assert_eq!(code, "transaction.currency-mismatch");
    assert_eq!(message, "账户币种（USD）与交易币种（CNY）不一致");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "守卫拒绝后不落行");

    let conn = test_support::open();
    seed_account(&conn, "acc-cny", "现金", "cash", "CNY", 0);
    seed_account(&conn, "acc-usd", "美元户", "cash", "USD", 0);
    let mut row = carried_row(TransactionKind::Transfer, "acc-cny", 3_000);
    row.to_account_id = Some("acc-usd".into());
    let err = replay_command(
        &conn,
        &TransactionCommand::Create {
            id: "sync-tx-2".into(),
            row,
            investment: None,
            split: None,
            convert: None,
        },
    )
    .unwrap_err();
    assert_eq!(coded_of(err).0, "transaction.currency-mismatch");
}
