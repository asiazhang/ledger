use super::super::{DomainCommand, OpOutcome, read_ops};
use super::common::{make_expense, read_transaction, seed_device, wire_in, wire_out};
use ledger_currencies::{current_base_currency, set_base_currency};
use ledger_transaction::write::protocol;
use tauri_app_lib::test_support::{self, seed_account, seed_exchange_rate};

#[test]
fn concurrent_base_currency_edits_converge_to_order_last() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    set_base_currency(&conn_a, "USD").unwrap();
    set_base_currency(&conn_b, "EUR").unwrap();

    wire_in(&conn_b, &wire_out(&conn_a));
    wire_in(&conn_a, &wire_out(&conn_b));

    assert_eq!(current_base_currency(&conn_a).unwrap(), "EUR");
    assert_eq!(current_base_currency(&conn_b).unwrap(), "EUR");

    assert_eq!(read_ops(&conn_a).unwrap(), read_ops(&conn_b).unwrap());
    assert!(
        read_ops(&conn_a).unwrap().iter().any(|op| matches!(
            &op.command,
            DomainCommand::LedgerSetting(
                ledger_currencies::LedgerSettingCommand::SetBaseCurrency { code }
            ) if code == "USD"
        )),
        "输者的设置 op 仍可审计"
    );
}

#[test]
fn concurrent_base_currency_reports_lww_outcomes() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");

    set_base_currency(&conn_a, "USD").unwrap();
    set_base_currency(&conn_b, "EUR").unwrap();

    let reports_on_b = wire_in(&conn_b, &wire_out(&conn_a));
    let reports_on_a = wire_in(&conn_a, &wire_out(&conn_b));

    assert!(
        reports_on_b
            .iter()
            .any(|r| r.outcome == OpOutcome::Superseded),
        "A 的设置在 B 端应为 LWW 输者：{reports_on_b:?}"
    );
    assert!(
        reports_on_a.iter().any(|r| r.outcome == OpOutcome::Applied),
        "B 的设置在 A 端应为序末者执行：{reports_on_a:?}"
    );
}

#[test]
fn conversion_is_deterministic_across_devices_after_sync() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_a, "dev-a");
    seed_device(&conn_b, "dev-b");
    seed_account(&conn_a, "acc-1", "现金", "cash", "EUR", 0);
    seed_account(&conn_b, "acc-1", "现金", "cash", "EUR", 0);

    set_base_currency(&conn_a, "USD").unwrap();
    wire_in(&conn_b, &wire_out(&conn_a));
    assert_eq!(current_base_currency(&conn_b).unwrap(), "USD");

    seed_exchange_rate(&conn_a, "EUR", "USD", 1.1);
    seed_exchange_rate(&conn_b, "EUR", "USD", 1.1);
    test_support::seed_fx_rate_history(&conn_a, "fxh-ls-a", "EUR", "USD", "2026-01-05", 1.1);
    test_support::seed_fx_rate_history(&conn_b, "fxh-ls-b", "EUR", "USD", "2026-01-05", 1.1);
    let mut input = make_expense("acc-1", 10000, "外币支出");
    input.currency_code = "EUR".into();
    let id_a = protocol::create(&conn_a, input.clone()).unwrap().id;
    let id_b = protocol::create(&conn_b, input).unwrap().id;

    let row_a = read_transaction(&conn_a, &id_a).unwrap();
    let row_b = read_transaction(&conn_b, &id_b).unwrap();
    assert_eq!(row_a.amount_native_cents, 11000);
    assert_eq!(row_b.amount_native_cents, 11000);
}

#[test]
fn base_currency_op_replay_is_idempotent() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();
    seed_device(&conn_b, "dev-b");

    set_base_currency(&conn_a, "USD").unwrap();
    let wire = wire_out(&conn_a);
    wire_in(&conn_b, &wire);
    assert_eq!(current_base_currency(&conn_b).unwrap(), "USD");

    let reports = wire_in(&conn_b, &wire);
    assert!(
        reports.iter().all(|r| r.outcome == OpOutcome::Skipped),
        "重复投递应全部跳过：{reports:?}"
    );
    assert_eq!(current_base_currency(&conn_b).unwrap(), "USD");
}
