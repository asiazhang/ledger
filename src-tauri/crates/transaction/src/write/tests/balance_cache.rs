use rusqlite::{Connection, params};

use crate::tests::common::{make_buy_input, make_input};
use ledger_accounts::balance::{compute_all_balances_with_visibility, compute_balance};
use ledger_accounts::{
    AccountBalanceAdjustInput, AccountInput, AccountType, adjust_account_balance,
    audit_balance_cache, create_account, delete_account, list_account_balances_for_api,
    list_account_balances_with_visibility as domain_list_balances,
};
use ledger_dashboard::query_dashboard_overview;
use tauri_app_lib::ledger_transaction::TransactionBatch;
use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::ledger_transaction::write::writer;
use tauri_app_lib::ledger_transaction::*;
use tauri_app_lib::test_support;
use tauri_app_lib::test_support::assert_balance_cache_matches_realtime;

fn backfill_scaffold_account(conn: &Connection, account_id: &str) {
    ledger_accounts::balance::refresh_account_balances(conn, &[account_id]).unwrap();
}

#[test]
fn create_all_kinds_keep_cache_consistent() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-cash", "现金", "cash", "CNY", 0);
    test_support::seed_investment_setup(&conn, "acc-inv", "inst-k");
    backfill_scaffold_account(&conn, "acc-cash");
    backfill_scaffold_account(&conn, "acc-inv");

    create_transaction_internal(
        &conn,
        make_input("acc-cash", TransactionKind::Income, 10000, "2026-01-01"),
    )
    .unwrap();
    assert_balance_cache_matches_realtime(&conn);

    create_transaction_internal(
        &conn,
        make_input("acc-cash", TransactionKind::Expense, 2000, "2026-01-02"),
    )
    .unwrap();
    assert_balance_cache_matches_realtime(&conn);

    create_account(
        &conn,
        AccountInput {
            name: "目标".into(),
            kind: AccountType::Bank,
            currency_code: "CNY".into(),
            initial_balance_cents: None,
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .unwrap();
    let to_id: String = conn
        .query_row("SELECT id FROM accounts WHERE name='目标'", [], |r| {
            r.get(0)
        })
        .unwrap();
    create_transaction_internal(
        &conn,
        TransactionInput {
            to_account_id: Some(to_id.clone()),
            ..make_input("acc-cash", TransactionKind::Transfer, 500, "2026-01-03")
        },
    )
    .unwrap();
    assert_balance_cache_matches_realtime(&conn);

    let expense_id = create_transaction_internal(
        &conn,
        make_input("acc-cash", TransactionKind::Expense, 300, "2026-01-04"),
    )
    .unwrap()
    .id;
    create_transaction_internal(
        &conn,
        TransactionInput {
            refund_of_transaction_id: Some(expense_id),
            ..make_input("acc-cash", TransactionKind::Refund, 100, "2026-01-05")
        },
    )
    .unwrap();
    assert_balance_cache_matches_realtime(&conn);

    create_transaction_internal(&conn, make_buy_input("acc-inv", "inst-k", 2.0, 100000, 100))
        .unwrap();
    assert_balance_cache_matches_realtime(&conn);
    let mut sell = make_buy_input("acc-inv", "inst-k", 1.0, 110000, 0);
    sell.kind = TransactionKind::Sell;
    sell.date = "2026-01-06".into();
    create_transaction_internal(&conn, sell).unwrap();
    assert_balance_cache_matches_realtime(&conn);
}

#[test]
fn update_cross_account_refreshes_old_and_new_union() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-u1", "甲", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-u2", "乙", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-u3", "丙", "cash", "CNY", 0);
    for id in ["acc-u1", "acc-u2", "acc-u3"] {
        backfill_scaffold_account(&conn, id);
    }

    let id = create_transaction_internal(
        &conn,
        make_input("acc-u1", TransactionKind::Expense, 400, "2026-01-01"),
    )
    .unwrap()
    .id;
    assert_balance_cache_matches_realtime(&conn);

    update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            to_account_id: Some("acc-u2".into()),
            ..make_input("acc-u1", TransactionKind::Transfer, 400, "2026-01-01")
        },
    )
    .unwrap();
    assert_balance_cache_matches_realtime(&conn);

    update_transaction_internal(
        &conn,
        &id,
        make_input("acc-u3", TransactionKind::Expense, 900, "2026-01-02"),
    )
    .unwrap();
    assert_balance_cache_matches_realtime(&conn);
}

#[test]
fn foreign_currency_balance_stays_in_account_currency() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-fx", "港币现金", "cash", "HKD", 0);
    backfill_scaffold_account(&conn, "acc-fx");
    test_support::seed_fx_history_weeks(&conn, "HKD", "CNY", 0.814145, &["2026-01-15"]);

    create_transaction_internal(
        &conn,
        TransactionInput {
            currency_code: "HKD".into(),
            ..make_input("acc-fx", TransactionKind::Income, 19091, "2026-01-15")
        },
    )
    .unwrap();

    assert_balance_cache_matches_realtime(&conn);
    assert_eq!(
        compute_balance(&conn, "acc-fx").unwrap(),
        19091,
        "余额应为账户币种原币金额，而非折算后的本位币值"
    );
    let native: i64 = conn
        .query_row(
            "SELECT amount_native_cents FROM transactions WHERE account_id='acc-fx'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_ne!(native, 19091, "夹具应产生非 1:1 折算值，实际: {native}");
}

#[test]
fn delete_transfer_restores_both_sides() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-d1", "甲", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-d2", "乙", "cash", "CNY", 0);
    for id in ["acc-d1", "acc-d2"] {
        backfill_scaffold_account(&conn, id);
    }

    let id = create_transaction_internal(
        &conn,
        TransactionInput {
            to_account_id: Some("acc-d2".into()),
            ..make_input("acc-d1", TransactionKind::Transfer, 700, "2026-01-01")
        },
    )
    .unwrap()
    .id;
    assert_balance_cache_matches_realtime(&conn);
    assert_eq!(compute_balance(&conn, "acc-d1").unwrap(), -700);
    assert_eq!(compute_balance(&conn, "acc-d2").unwrap(), 700);

    delete_transaction_internal(&conn, &id).unwrap();
    assert_balance_cache_matches_realtime(&conn);
    assert_eq!(compute_balance(&conn, "acc-d1").unwrap(), 0);
    assert_eq!(compute_balance(&conn, "acc-d2").unwrap(), 0);
}

fn seed_sell_funding_fixture(conn: &Connection) {
    test_support::seed_investment_setup(conn, "acc-inv", "inst-sf");
    test_support::seed_account(conn, "acc-fund-a", "出资卡A", "bank", "USD", 0);
    test_support::seed_account(conn, "acc-fund-b", "出资卡B", "bank", "USD", 0);
    for id in ["acc-inv", "acc-fund-a", "acc-fund-b"] {
        backfill_scaffold_account(conn, id);
    }
    create_transaction_internal(conn, make_buy_input("acc-inv", "inst-sf", 2.0, 1000000, 0))
        .unwrap();
}

fn make_sell_input(funding_account_id: Option<&str>) -> TransactionInput {
    let mut sell = make_buy_input("acc-inv", "inst-sf", 2.0, 1000000, 0);
    sell.kind = TransactionKind::Sell;
    sell.funding_account_id = funding_account_id.map(str::to_string);
    sell
}

#[test]
fn create_sell_with_funding_credits_funding_not_investment() {
    let conn = test_support::open();
    seed_sell_funding_fixture(&conn);
    let inv_before = compute_balance(&conn, "acc-inv").unwrap();

    create_transaction_internal(&conn, make_sell_input(Some("acc-fund-a"))).unwrap();

    assert_balance_cache_matches_realtime(&conn);
    assert_eq!(
        compute_balance(&conn, "acc-fund-a").unwrap(),
        20000,
        "回卡卖出：赎回款 + 于出资账户"
    );
    assert_eq!(
        compute_balance(&conn, "acc-inv").unwrap(),
        inv_before,
        "投资账户现金不动（出资账户命中时现金腿为 0）"
    );
}

#[test]
fn create_sell_without_funding_keeps_balance_sell_semantics() {
    let conn = test_support::open();
    seed_sell_funding_fixture(&conn);

    create_transaction_internal(&conn, make_sell_input(None)).unwrap();

    assert_balance_cache_matches_realtime(&conn);
    assert_eq!(
        compute_balance(&conn, "acc-inv").unwrap(),
        0,
        "不填出资账户：买入 − 与卖出 + 同落投资账户（子弹口径回到 0）"
    );
    assert_eq!(compute_balance(&conn, "acc-fund-a").unwrap(), 0);
}

#[test]
fn update_sell_funding_swap_recomputes_three_ends() {
    let conn = test_support::open();
    seed_sell_funding_fixture(&conn);
    let inv_before = compute_balance(&conn, "acc-inv").unwrap();
    let id = create_transaction_internal(&conn, make_sell_input(Some("acc-fund-a")))
        .unwrap()
        .id;

    update_transaction_internal(&conn, &id, make_sell_input(Some("acc-fund-b"))).unwrap();

    assert_balance_cache_matches_realtime(&conn);
    assert_eq!(
        compute_balance(&conn, "acc-fund-a").unwrap(),
        0,
        "旧出资端随重算恢复"
    );
    assert_eq!(
        compute_balance(&conn, "acc-fund-b").unwrap(),
        20000,
        "新出资端计入赎回款"
    );
    assert_eq!(compute_balance(&conn, "acc-inv").unwrap(), inv_before);
}

#[test]
fn delete_sell_with_funding_restores_funding_balance() {
    let conn = test_support::open();
    seed_sell_funding_fixture(&conn);
    let inv_before = compute_balance(&conn, "acc-inv").unwrap();
    let id = create_transaction_internal(&conn, make_sell_input(Some("acc-fund-a")))
        .unwrap()
        .id;

    delete_transaction_internal(&conn, &id).unwrap();

    assert_balance_cache_matches_realtime(&conn);
    assert_eq!(
        compute_balance(&conn, "acc-fund-a").unwrap(),
        0,
        "删除回补出资账户，不留幽灵入账"
    );
    assert_eq!(compute_balance(&conn, "acc-inv").unwrap(), inv_before);
}

#[test]
fn batch_import_keeps_cache_consistent() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-batch", "现金", "cash", "CNY", 0);
    backfill_scaffold_account(&conn, "acc-batch");

    TransactionBatch::run(
        &conn,
        vec![
            make_input("acc-batch", TransactionKind::Income, 8000, "2026-02-01"),
            make_input("acc-batch", TransactionKind::Expense, 2500, "2026-02-02"),
        ],
        true,
    )
    .unwrap();
    assert_balance_cache_matches_realtime(&conn);
}

#[test]
fn writer_insert_row_direct_seam_refreshes_cache() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-eng", "现金", "cash", "CNY", 0);
    backfill_scaffold_account(&conn, "acc-eng");

    let input = writer::Input {
        funding: Vec::new(),
        existing_funding: Vec::new(),
        kind: TransactionKind::Income,
        amount_cents: 1200,
        currency_code: "CNY".into(),
        account_id: Some("acc-eng".into()),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        existing_merchant_id: None,
        policy_id: None,
        existing_policy_id: None,
        refund_of_transaction_id: None,
        funding_account_id: None,
        source_order_no: None,
        note: None,
        date: "2026-03-01".into(),
        fx_rate: None,
        fx_edit_baseline: None,
        purchases: Vec::new(),
        existing_purchases: Vec::new(),
    };
    let row = writer::normalize(&conn, &input).unwrap();
    writer::insert_row(&conn, &row).unwrap();
    assert_balance_cache_matches_realtime(&conn);
    assert_eq!(compute_balance(&conn, "acc-eng").unwrap(), 1200);
}

#[test]
fn account_create_and_delete_maintain_cache_rows() {
    let conn = test_support::open();
    let id = create_account(
        &conn,
        AccountInput {
            name: "钱包".into(),
            kind: AccountType::Ewallet,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(6800),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .unwrap();
    let cached: i64 = conn
        .query_row(
            "SELECT balance_cents FROM account_balance_cache WHERE account_id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cached, 6800, "创建账户应即建缓存行（初始余额）");

    delete_account(&conn, &id).unwrap();
    assert_balance_cache_matches_realtime(&conn);
}

#[test]
fn adjust_balance_targets_exact_value_via_cache() {
    let conn = test_support::open();
    let id = create_account(
        &conn,
        AccountInput {
            name: "工资卡".into(),
            kind: AccountType::Bank,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(1000),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .unwrap();

    let adjust = |target: i64| {
        adjust_account_balance(
            &conn,
            &id,
            &AccountBalanceAdjustInput {
                target_balance_cents: target,
                date: "2026-04-01".into(),
                note: None,
            },
        )
        .unwrap()
    };
    adjust(5600);
    assert_balance_cache_matches_realtime(&conn);
    assert_eq!(compute_balance(&conn, &id).unwrap(), 5600);

    adjust(4300);
    assert_balance_cache_matches_realtime(&conn);
    assert_eq!(compute_balance(&conn, &id).unwrap(), 4300);

    assert_balance_cache_matches_realtime(&conn);
}

#[test]
fn five_outlets_return_realtime_consistent_values() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-o1", "现金", "cash", "USD", 0);
    test_support::seed_investment_setup(&conn, "acc-o2", "inst-o");
    backfill_scaffold_account(&conn, "acc-o1");
    backfill_scaffold_account(&conn, "acc-o2");
    create_transaction_internal(
        &conn,
        TransactionInput {
            currency_code: "USD".into(),
            ..make_input("acc-o1", TransactionKind::Income, 1000000, "2026-05-01")
        },
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        TransactionInput {
            currency_code: "USD".into(),
            to_account_id: Some("acc-o2".into()),
            ..make_input("acc-o1", TransactionKind::Transfer, 600000, "2026-05-02")
        },
    )
    .unwrap();
    create_transaction_internal(&conn, make_buy_input("acc-o2", "inst-o", 1.0, 400000, 0)).unwrap();

    let realtime_visible = compute_all_balances_with_visibility(&conn, false).unwrap();
    for ab in domain_list_balances(&conn, false).unwrap() {
        assert_eq!(
            Some(ab.balance_cents),
            realtime_visible.get(&ab.account.id).copied(),
            "UI 清单出口应与实时一致: {}",
            ab.account.id
        );
    }
    let realtime_all = compute_all_balances_with_visibility(&conn, true).unwrap();
    for ab in list_account_balances_for_api(&conn).unwrap() {
        assert_eq!(
            Some(ab.balance_cents),
            realtime_all.get(&ab.account.id).copied(),
            "HTTP/AI 清单出口应与实时一致: {}",
            ab.account.id
        );
    }

    let overview = query_dashboard_overview(&conn).unwrap();
    let expected_accounts_sum: i64 = realtime_visible.values().sum();
    assert_eq!(
        overview.accounts_balance_cents, expected_accounts_sum,
        "dashboard 余额腿应与实时合计一致"
    );
    assert_eq!(
        overview.accounts_balance_cents, 996000,
        "余额腿应计入投资账户现金（acc-o1 400000 + acc-o2 现金 596000）"
    );
    assert_eq!(
        overview.net_worth_cents,
        overview.accounts_balance_cents
            + overview.holdings_market_value_cents
            + overview.physical_assets_value_cents
    );

    assert_eq!(
        ledger_accounts::balance::cached_balance(&conn, "acc-o1").unwrap(),
        compute_balance(&conn, "acc-o1").unwrap()
    );

    let freedom = ledger_investment::query_financial_freedom(&conn).unwrap();
    assert_eq!(
        freedom.numerator_cents, 596000,
        "财务自由度分子应与实时口径一致（投资账户现金腿读缓存）"
    );
}

#[test]
fn missing_cache_row_raises_coded_error() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-miss", "现金", "cash", "CNY", 0);
    backfill_scaffold_account(&conn, "acc-miss");

    conn.execute(
        "DELETE FROM account_balance_cache WHERE account_id='acc-miss'",
        [],
    )
    .unwrap();
    let err = domain_list_balances(&conn, false).unwrap_err();
    assert!(
        err.to_string().contains("余额缓存审计"),
        "缺失缓存行应引导审计修复，实际: {err:?}"
    );
}

#[test]
fn net_worth_probe_backfills_hits_and_self_heals() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-p1", "现金", "cash", "CNY", 0);
    backfill_scaffold_account(&conn, "acc-p1");
    create_transaction_internal(
        &conn,
        make_input("acc-p1", TransactionKind::Income, 10000, "2026-06-01"),
    )
    .unwrap();

    let first = query_dashboard_overview(&conn).unwrap();
    assert_eq!(first.net_worth_cents, 10000);
    let fp = ledger_dashboard::net_worth::current_fingerprint(&conn).unwrap();
    let cached = ledger_dashboard::net_worth::read_valid(&conn, &fp).unwrap();
    assert!(cached.is_some(), "首读后应有指纹匹配的缓存行");
    assert_eq!(cached.unwrap().net_worth_cents, 10000);

    conn.execute(
        "UPDATE net_worth_cache SET net_worth_cents = 777 WHERE id = 1",
        [],
    )
    .unwrap();
    let hit = query_dashboard_overview(&conn).unwrap();
    assert_eq!(hit.net_worth_cents, 777, "指纹命中应直读缓存终值");

    create_transaction_internal(
        &conn,
        make_input("acc-p1", TransactionKind::Expense, 2500, "2026-06-02"),
    )
    .unwrap();
    let healed = query_dashboard_overview(&conn).unwrap();
    assert_eq!(
        healed.net_worth_cents, 7500,
        "源变更后应重算自愈，不再返回陈旧缓存"
    );
}

#[test]
fn audit_polluted_cache_reports_then_repairs() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a1", "甲", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-a2", "乙", "cash", "CNY", 0);
    for id in ["acc-a1", "acc-a2"] {
        backfill_scaffold_account(&conn, id);
    }
    create_transaction_internal(
        &conn,
        TransactionInput {
            to_account_id: Some("acc-a2".into()),
            ..make_input("acc-a1", TransactionKind::Transfer, 1500, "2026-07-01")
        },
    )
    .unwrap();

    conn.execute(
        "UPDATE account_balance_cache SET balance_cents = balance_cents + 100 WHERE account_id='acc-a1'",
        [],
    )
    .unwrap();
    conn.execute(
        "DELETE FROM account_balance_cache WHERE account_id='acc-a2'",
        [],
    )
    .unwrap();

    let report = audit_balance_cache(&conn).unwrap();
    assert!(report.repaired, "存在差异应触发修复");
    assert_eq!(report.accounts_checked, 4);
    assert_eq!(report.drifts.len(), 2, "漂移与缺行都应入报告");
    let missing = report
        .drifts
        .iter()
        .find(|d| d.account_id == "acc-a2")
        .expect("缺行账户应入差异报告");
    assert_eq!(missing.cached_cents, None, "缓存缺失记 None");
    assert_eq!(missing.actual_cents, 1500);

    assert_balance_cache_matches_realtime(&conn);
    let recheck = audit_balance_cache(&conn).unwrap();
    assert!(!recheck.repaired, "修复后复检应无差异");
    assert!(recheck.drifts.is_empty());
}
