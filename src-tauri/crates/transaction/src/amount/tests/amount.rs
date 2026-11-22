use ledger_infra::error::AppError;
use rusqlite::Connection;
use rusqlite::params;

use tauri_app_lib::ledger_transaction::amount::*;
use tauri_app_lib::test_support;

fn insert_txn(
    conn: &Connection,
    id: &str,
    kind: TransactionKind,
    amount_native_cents: i64,
    account_id: &str,
    to_account_id: Option<&str>,
) {
    insert_txn_dual(
        conn,
        id,
        kind,
        amount_native_cents,
        amount_native_cents,
        account_id,
        to_account_id,
    );
}

fn insert_txn_dual(
    conn: &Connection,
    id: &str,
    kind: TransactionKind,
    amount_cents: i64,
    amount_native_cents: i64,
    account_id: &str,
    to_account_id: Option<&str>,
) {
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,date,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,'HKD',?4,?5,?6,'2026-02-01','2026-02-01T00:00:00Z','2026-02-01T00:00:00Z',1,'test')",
        params![id, kind.as_str(), amount_cents, amount_native_cents, account_id, to_account_id],
    )
    .unwrap();
}

#[test]
fn kind_string_roundtrip() {
    assert_eq!(TransactionKind::ALL.len(), 9);
    for kind in TransactionKind::ALL {
        assert_eq!(TransactionKind::parse(kind.as_str()).unwrap(), kind);
        assert_eq!(kind.to_string(), kind.as_str());
    }
}

#[test]
fn kind_serde_roundtrip() {
    for kind in TransactionKind::ALL {
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, format!("\"{}\"", kind.as_str()));
        let back: TransactionKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, kind);
    }
    let err = serde_json::from_str::<TransactionKind>("\"bonus\"").unwrap_err();
    assert!(err.to_string().contains("未知交易类型"), "实际: {err}");
}

#[test]
fn kind_parse_rejects_unknown() {
    let err = TransactionKind::parse("bonus").unwrap_err();
    assert_eq!(err.code(), Some("transaction.kind-unknown"));
    assert_eq!(
        err.to_string(),
        "未知交易类型: bonus（合法值: income/expense/transfer/refund/buy/sell/dividend/split/convert）",
        "message 逐字不变（ADR-0050 只增不改）"
    );
    assert_eq!(
        serde_json::to_value(&err).unwrap()["params"],
        serde_json::json!([
            "bonus",
            "income/expense/transfer/refund/buy/sell/dividend/split/convert"
        ]),
        "params 按动态值出现顺序：未知值 → 同源合法值清单"
    );
    assert!(TransactionKind::parse("").is_err());
}

#[test]
fn matrix_signed_amount_all_cells() {
    use Measure::*;
    use TransactionKind::*;
    let cells: &[(TransactionKind, Measure, i64)] = &[
        (Income, AccountFlow(TransferSide::Out), 1),
        (Expense, AccountFlow(TransferSide::Out), -1),
        (Transfer, AccountFlow(TransferSide::Out), -1),
        (Refund, AccountFlow(TransferSide::Out), 1),
        (Buy, AccountFlow(TransferSide::Out), -1),
        (Sell, AccountFlow(TransferSide::Out), 1),
        (Dividend, AccountFlow(TransferSide::Out), 1),
        (Split, AccountFlow(TransferSide::Out), 0),
        (Convert, AccountFlow(TransferSide::Out), 0),
        (Income, AccountFlow(TransferSide::In), 1),
        (Expense, AccountFlow(TransferSide::In), -1),
        (Transfer, AccountFlow(TransferSide::In), 1),
        (Refund, AccountFlow(TransferSide::In), 1),
        (Buy, AccountFlow(TransferSide::In), -1),
        (Sell, AccountFlow(TransferSide::In), 1),
        (Dividend, AccountFlow(TransferSide::In), 1),
        (Split, AccountFlow(TransferSide::In), 0),
        (Convert, AccountFlow(TransferSide::In), 0),
        (Income, ExpenseNet, 0),
        (Expense, ExpenseNet, 1),
        (Transfer, ExpenseNet, 0),
        (Refund, ExpenseNet, -1),
        (Buy, ExpenseNet, 0),
        (Sell, ExpenseNet, 0),
        (Dividend, ExpenseNet, 0),
        (Split, ExpenseNet, 0),
        (Convert, ExpenseNet, 0),
        (Income, IncomeNet, 1),
        (Expense, IncomeNet, 0),
        (Transfer, IncomeNet, 0),
        (Refund, IncomeNet, 0),
        (Buy, IncomeNet, 0),
        (Sell, IncomeNet, 0),
        (Dividend, IncomeNet, 1),
        (Split, IncomeNet, 0),
        (Convert, IncomeNet, 0),
        (Income, RefundGross, 0),
        (Expense, RefundGross, 0),
        (Transfer, RefundGross, 0),
        (Refund, RefundGross, 1),
        (Buy, RefundGross, 0),
        (Sell, RefundGross, 0),
        (Dividend, RefundGross, 0),
        (Split, RefundGross, 0),
        (Convert, RefundGross, 0),
        (Income, PolicyPremium, 0),
        (Expense, PolicyPremium, 1),
        (Transfer, PolicyPremium, 0),
        (Refund, PolicyPremium, 0),
        (Buy, PolicyPremium, 0),
        (Sell, PolicyPremium, 0),
        (Dividend, PolicyPremium, 0),
        (Split, PolicyPremium, 0),
        (Convert, PolicyPremium, 0),
        (Income, PolicyInflow, 1),
        (Expense, PolicyInflow, 0),
        (Transfer, PolicyInflow, 0),
        (Refund, PolicyInflow, 0),
        (Buy, PolicyInflow, 0),
        (Sell, PolicyInflow, 0),
        (Dividend, PolicyInflow, 0),
        (Split, PolicyInflow, 0),
        (Convert, PolicyInflow, 0),
    ];
    for &(kind, measure, expect_sign) in cells {
        assert_eq!(
            signed_amount(kind, 700, measure),
            expect_sign * 700,
            "kind={kind:?} measure={measure:?}"
        );
    }
}

fn sql_sum(conn: &Connection, expr: &str) -> i64 {
    conn.query_row(
        &format!("SELECT COALESCE(SUM({expr}),0) FROM transactions t WHERE t.is_deleted=0"),
        [],
        |r| r.get(0),
    )
    .unwrap()
}

fn rust_sum(conn: &Connection, measure: Measure) -> i64 {
    let mut stmt = conn
        .prepare(
            "SELECT kind, amount_cents, amount_native_cents FROM transactions WHERE is_deleted=0",
        )
        .unwrap();
    let rows: Vec<(TransactionKind, i64, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    rows.into_iter()
        .map(|(k, amount, native)| {
            let amt = if matches!(measure, Measure::AccountFlow(_)) {
                amount
            } else {
                native
            };
            signed_amount(k, amt, measure)
        })
        .sum()
}

#[test]
fn sql_exprs_match_rust_sums() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "HKD", 0);
    for (i, kind) in TransactionKind::ALL.into_iter().enumerate() {
        insert_txn_dual(
            &conn,
            &format!("t-{i}"),
            kind,
            100 + i as i64 * 7,
            1000 + i as i64 * 13,
            "acc",
            None,
        );
    }

    let measures = [
        Measure::AccountFlow(TransferSide::Out),
        Measure::AccountFlow(TransferSide::In),
        Measure::ExpenseNet,
        Measure::IncomeNet,
        Measure::RefundGross,
        Measure::PolicyPremium,
        Measure::PolicyInflow,
    ];
    for measure in measures {
        let expr = match measure {
            Measure::AccountFlow(side) => account_flow_expr("t", side),
            Measure::ExpenseNet => expense_net_expr("t"),
            Measure::IncomeNet => income_net_expr("t"),
            Measure::RefundGross => refund_gross_expr("t"),
            Measure::PolicyPremium => policy_premium_expr("t"),
            Measure::PolicyInflow => policy_inflow_expr("t"),
        };
        assert_eq!(
            sql_sum(&conn, &expr),
            rust_sum(&conn, measure),
            "SQL 片段与 Rust 聚合不一致: {measure:?} => {expr}"
        );
    }
}

#[test]
fn expense_gross_expr_equals_net_plus_refund_gross() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    for (i, kind) in TransactionKind::ALL.into_iter().enumerate() {
        insert_txn(
            &conn,
            &format!("t-{i}"),
            kind,
            100 + i as i64 * 7,
            "acc",
            None,
        );
    }
    assert_eq!(
        sql_sum(&conn, &expense_gross_expr("t")),
        sql_sum(&conn, &expense_net_expr("t")) + sql_sum(&conn, &refund_gross_expr("t")),
        "毛支出恒等式在真实库上应逐分成立"
    );
}

#[test]
fn contributing_kinds_follow_matrix() {
    assert_eq!(
        contributing_kinds(Measure::ExpenseNet),
        vec!["expense", "refund"]
    );
    assert_eq!(
        contributing_kinds(Measure::IncomeNet),
        vec!["income", "dividend"]
    );
    assert_eq!(contributing_kinds(Measure::RefundGross), vec!["refund"]);
    assert_eq!(contributing_kinds(Measure::PolicyPremium), vec!["expense"]);
    assert_eq!(contributing_kinds(Measure::PolicyInflow), vec!["income"]);
    assert_eq!(
        contributing_kinds(Measure::AccountFlow(TransferSide::Out)),
        vec![
            "income", "expense", "transfer", "refund", "buy", "sell", "dividend"
        ]
    );
    assert_eq!(
        contributing_kinds(Measure::AccountFlow(TransferSide::In)),
        contributing_kinds(Measure::AccountFlow(TransferSide::Out))
    );
}

#[test]
fn account_flow_expr_balances_match_rust() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-b", "acc-b", "cash", "CNY", 0);

    insert_txn(&conn, "t1", TransactionKind::Income, 5000, "acc-a", None);
    insert_txn(&conn, "t2", TransactionKind::Expense, 1200, "acc-a", None);
    insert_txn(&conn, "t3", TransactionKind::Refund, 300, "acc-a", None);
    insert_txn(&conn, "t4", TransactionKind::Buy, 2000, "acc-a", None);
    insert_txn(&conn, "t5", TransactionKind::Split, 9999, "acc-a", None);
    insert_txn(&conn, "t5c", TransactionKind::Convert, 4321, "acc-a", None);
    insert_txn(
        &conn,
        "t6",
        TransactionKind::Transfer,
        800,
        "acc-a",
        Some("acc-b"),
    );
    insert_txn(&conn, "t7", TransactionKind::Dividend, 60, "acc-b", None);

    let balance_sql = |account: &str| -> i64 {
        let out: i64 = conn
            .query_row(
                &format!(
                    "SELECT COALESCE(SUM({}),0) FROM transactions t \
                     WHERE t.is_deleted=0 AND t.account_id=?1",
                    account_flow_expr("t", TransferSide::Out)
                ),
                params![account],
                |r| r.get(0),
            )
            .unwrap();
        let incoming: i64 = conn
            .query_row(
                &format!(
                    "SELECT COALESCE(SUM({}),0) FROM transactions t \
                     WHERE t.is_deleted=0 AND t.to_account_id=?1",
                    account_flow_expr("t", TransferSide::In)
                ),
                params![account],
                |r| r.get(0),
            )
            .unwrap();
        out + incoming
    };

    let balance_rust = |account: &str| -> i64 {
        let mut stmt = conn
            .prepare(
                "SELECT kind, amount_cents, account_id, to_account_id \
                 FROM transactions WHERE is_deleted=0",
            )
            .unwrap();
        let rows: Vec<(TransactionKind, i64, String, Option<String>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        rows.into_iter()
            .filter(|(_, _, acc, to)| {
                acc == account || (to.is_some() && to.as_deref() == Some(account))
            })
            .map(|(kind, amt, acc, _to)| {
                let side = if acc == account {
                    TransferSide::Out
                } else {
                    TransferSide::In
                };
                signed_amount(kind, amt, Measure::AccountFlow(side))
            })
            .sum()
    };

    assert_eq!(balance_rust("acc-a"), 1300);
    assert_eq!(balance_rust("acc-b"), 860);
    assert_eq!(balance_sql("acc-a"), balance_rust("acc-a"));
    assert_eq!(balance_sql("acc-b"), balance_rust("acc-b"));
}

#[test]
fn convert_to_native_current_same_currency_is_identity() {
    let conn = test_support::open();
    assert_eq!(
        convert_to_native_current(&conn, 12345, &default_currency_code(&conn).unwrap()).unwrap(),
        12345
    );
}

#[test]
fn default_currency_code_reads_ledger_setting() {
    let conn = test_support::open();
    assert_eq!(default_currency_code(&conn).unwrap(), "CNY");
    ledger_currencies::set_base_currency(&conn, "USD").unwrap();
    assert_eq!(default_currency_code(&conn).unwrap(), "USD");
}

#[test]
fn convert_to_native_current_follows_base_currency_setting() {
    let conn = test_support::open();
    test_support::seed_exchange_rate(&conn, "EUR", "USD", 1.1);
    ledger_currencies::set_base_currency(&conn, "USD").unwrap();
    assert_eq!(
        convert_to_native_current(&conn, 10000, "EUR").unwrap(),
        11000
    );
}

#[test]
fn convert_to_native_current_uses_rate_to_default_currency() {
    let conn = test_support::open();
    test_support::seed_exchange_rate(&conn, "USD", "CNY", 7.2);
    assert_eq!(
        convert_to_native_current(&conn, 10000, "USD").unwrap(),
        72000
    );
}

#[test]
fn convert_to_native_current_is_independent_of_account_currency() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-usd", "acc-usd", "cash", "USD", 0);
    test_support::seed_exchange_rate(&conn, "USD", "CNY", 7.2);
    assert_eq!(
        convert_to_native_current(&conn, 10000, "USD").unwrap(),
        72000
    );
}

#[test]
fn convert_to_native_current_uses_reverse_rate_when_only_reverse_exists() {
    let conn = test_support::open();
    test_support::seed_exchange_rate(&conn, "CNY", "EUR", 0.13);
    assert_eq!(
        convert_to_native_current(&conn, 10000, "EUR").unwrap(),
        76923
    );
}

#[test]
fn convert_to_native_current_errors_without_rate() {
    let conn = test_support::open();
    assert!(convert_to_native_current(&conn, 10000, "JPY").is_err());
}

#[test]
fn convert_to_native_current_rejects_non_positive_rate() {
    let conn = test_support::open();
    test_support::seed_exchange_rate(&conn, "USD", "CNY", 0.0);
    assert!(convert_to_native_current(&conn, 10000, "USD").is_err());

    let conn = test_support::open();
    test_support::seed_exchange_rate(&conn, "CNY", "EUR", -0.13);
    assert!(convert_to_native_current(&conn, 10000, "EUR").is_err());
}

#[test]
fn convert_to_native_current_ignores_fx_rate_history() {
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-r", "USD", "CNY", "2026-01-05", 7.5);
    assert!(convert_to_native_current(&conn, 10000, "USD").is_err());

    test_support::seed_exchange_rate(&conn, "USD", "CNY", 7.2);
    assert_eq!(
        convert_to_native_current(&conn, 10000, "USD").unwrap(),
        72000
    );
}

#[test]
fn convert_to_native_on_trade_date_same_currency_is_identity_matches_current() {
    let conn = test_support::open();
    let conv = convert_to_native_on_trade_date(
        &conn,
        12345,
        &default_currency_code(&conn).unwrap(),
        "2026-01-07",
        None,
    )
    .unwrap();
    assert_eq!(conv.native_cents, 12345);
    assert_eq!(conv.fx_rate_used, None, "同币种不折算：无汇率留痕");
    assert_eq!(conv.fx_rate_source, None, "同币种不折算：无来源留痕");
}

#[test]
fn convert_to_native_on_trade_date_uses_week_rate_of_trade_date() {
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-w1", "USD", "CNY", "2026-01-05", 7.5);
    test_support::seed_fx_rate_history(&conn, "fxh-w2", "USD", "CNY", "2026-01-12", 8.0);
    assert_eq!(
        convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", None)
            .unwrap()
            .native_cents,
        75000
    );
    assert_eq!(
        convert_to_native_on_trade_date(&conn, 12345, "USD", "2026-01-11", None)
            .unwrap()
            .native_cents,
        92588
    );
    assert_eq!(
        convert_to_native_on_trade_date(&conn, 12345, "USD", "2026-01-13", None)
            .unwrap()
            .native_cents,
        98760
    );
    test_support::seed_fx_rate_history(&conn, "fxh-hkd", "HKD", "CNY", "2026-01-05", 0.9);
    assert_eq!(
        convert_to_native_on_trade_date(&conn, 52_508_160, "HKD", "2026-01-07", None)
            .unwrap()
            .native_cents,
        47_257_344
    );
}

#[test]
fn convert_to_native_on_trade_date_traces_series_rate_and_source() {
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-fwd", "USD", "CNY", "2026-01-05", 7.5);
    let conv = convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", None).unwrap();
    assert_eq!(conv.fx_rate_used, Some(7.5));
    assert_eq!(conv.fx_rate_source, Some(FxRateSource::Series));
    assert_eq!(
        conv.native_cents, 75000,
        "native = amount × 留痕汇率（到分）"
    );

    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-rev", "CNY", "USD", "2026-01-05", 0.125);
    let conv = convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", None).unwrap();
    assert_eq!(conv.fx_rate_used, Some(8.0), "反向命中留痕存使用值（倒数）");
    assert_eq!(conv.fx_rate_source, Some(FxRateSource::Series));
    assert_eq!(conv.native_cents, 80000);
}

#[test]
fn fx_rate_source_closed_set_roundtrip_and_rejects_unknown() {
    assert_eq!(
        FxRateSource::ALL,
        [FxRateSource::Series, FxRateSource::Explicit]
    );
    for source in FxRateSource::ALL {
        assert_eq!(FxRateSource::parse(source.as_str()).unwrap(), source);
        let json = serde_json::to_string(&source).unwrap();
        assert_eq!(json, format!("\"{}\"", source.as_str()));
        let back: FxRateSource = serde_json::from_str(&json).unwrap();
        assert_eq!(back, source);
    }
    let err = FxRateSource::parse("guessed").unwrap_err();
    assert_eq!(err.code(), Some("fx.source-unknown"));
    assert_eq!(
        err.to_string(),
        "未知折算来源: guessed（合法值: series/explicit）",
        "message 逐字锁定（ADR-0050 只增不改）"
    );
}

#[test]
fn convert_to_native_on_trade_date_uses_reverse_rate_when_only_reverse_exists() {
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-rev", "CNY", "USD", "2026-01-06", 0.125);
    assert_eq!(
        convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", None)
            .unwrap()
            .native_cents,
        80000
    );
}

#[test]
fn convert_to_native_on_trade_date_errors_when_whole_week_missing() {
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-next", "USD", "CNY", "2026-01-12", 8.0);
    test_support::seed_exchange_rate(&conn, "USD", "CNY", 7.2);
    let err = convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", None).unwrap_err();
    match err {
        AppError::Coded { code, params, .. } => {
            assert_eq!(code, "fx.rate-missing");
            assert_eq!(params, vec!["USD".to_string(), "CNY".to_string()]);
        }
        other => panic!("应为中心化缺失码化错误，实际: {other}"),
    }
}

#[test]
fn convert_to_native_on_trade_date_missing_week_copy_distinguishes_current_from_gap() {
    let conn = test_support::open();
    let this_monday: String = conn
        .query_row(
            "SELECT date('now','localtime','-6 days','weekday 1')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let err = convert_to_native_on_trade_date(&conn, 10000, "USD", &this_monday, None).unwrap_err();
    match err {
        AppError::Coded { code, message, .. } => {
            assert_eq!(code, "fx.rate-missing");
            assert!(message.contains("尚未发布"), "实际: {message}");
            assert!(message.contains("重试"), "实际: {message}");
        }
        other => panic!("应为码化错误，实际: {other}"),
    }

    let conn = test_support::open();
    let err = convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", None).unwrap_err();
    match err {
        AppError::Coded { code, message, .. } => {
            assert_eq!(code, "fx.rate-missing");
            assert!(message.contains("历史空缺"), "实际: {message}");
            assert!(!message.contains("尚未发布"), "实际: {message}");
        }
        other => panic!("应为码化错误，实际: {other}"),
    }
}

#[test]
fn convert_to_native_on_trade_date_rejects_non_positive_rate() {
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-zero", "USD", "CNY", "2026-01-05", 0.0);
    assert!(convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", None).is_err());

    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-neg", "CNY", "USD", "2026-01-05", -0.13);
    assert!(convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", None).is_err());
}

fn reuse_baseline(
    amount_cents: i64,
    currency_code: &str,
    date: &str,
    conversion: NativeConversion,
) -> FxEditBaseline {
    FxEditBaseline {
        amount_cents,
        currency_code: currency_code.into(),
        date: date.into(),
        conversion,
    }
}

#[test]
fn convert_to_native_on_edit_reuses_inline_trace_when_triple_unchanged() {
    let conn = test_support::open();
    let baseline = reuse_baseline(
        1000,
        "HKD",
        "2026-01-07",
        NativeConversion {
            native_cents: 900,
            fx_rate_used: Some(0.9),
            fx_rate_source: Some(FxRateSource::Series),
        },
    );
    let conv =
        convert_to_native_on_edit(&conn, 1000, "HKD", "2026-01-07", None, Some(&baseline)).unwrap();
    assert_eq!(conv.native_cents, 900, "本位币金额逐位不变");
    assert_eq!(conv.fx_rate_used, Some(0.9), "汇率值逐位不变");
    assert_eq!(
        conv.fx_rate_source,
        Some(FxRateSource::Series),
        "来源逐位不变"
    );
}

#[test]
fn convert_to_native_on_edit_reuses_null_trace_when_triple_unchanged() {
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-legacy", "HKD", "CNY", "2026-01-05", 0.9);
    let baseline = reuse_baseline(
        1000,
        "HKD",
        "2026-01-07",
        NativeConversion {
            native_cents: 1000,
            fx_rate_used: None,
            fx_rate_source: None,
        },
    );
    let conv =
        convert_to_native_on_edit(&conn, 1000, "HKD", "2026-01-07", None, Some(&baseline)).unwrap();
    assert_eq!(conv.native_cents, 1000, "存量行金额不改写");
    assert_eq!(conv.fx_rate_used, None, "存量行空留痕不改写");
    assert_eq!(conv.fx_rate_source, None, "存量行空来源不改写");
}

#[test]
fn convert_to_native_on_edit_requeries_when_amount_changed() {
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-amt", "HKD", "CNY", "2026-01-05", 0.8);
    let baseline = reuse_baseline(
        1000,
        "HKD",
        "2026-01-07",
        NativeConversion {
            native_cents: 900,
            fx_rate_used: Some(0.9),
            fx_rate_source: Some(FxRateSource::Series),
        },
    );
    let conv =
        convert_to_native_on_edit(&conn, 2000, "HKD", "2026-01-07", None, Some(&baseline)).unwrap();
    assert_eq!(conv.native_cents, 1600, "按新金额与新序列值重算");
    assert_eq!(conv.fx_rate_used, Some(0.8), "留痕更新为本笔使用的序列值");
    assert_eq!(conv.fx_rate_source, Some(FxRateSource::Series));
}

#[test]
fn convert_to_native_on_edit_requeries_when_date_changed() {
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-w1", "HKD", "CNY", "2026-01-05", 0.9);
    test_support::seed_fx_rate_history(&conn, "fxh-w2", "HKD", "CNY", "2026-01-12", 0.95);
    let baseline = reuse_baseline(
        1000,
        "HKD",
        "2026-01-07",
        NativeConversion {
            native_cents: 900,
            fx_rate_used: Some(0.9),
            fx_rate_source: Some(FxRateSource::Series),
        },
    );
    let conv =
        convert_to_native_on_edit(&conn, 1000, "HKD", "2026-01-14", None, Some(&baseline)).unwrap();
    assert_eq!(conv.native_cents, 950, "按新日期所属周重算");
    assert_eq!(conv.fx_rate_used, Some(0.95));
    assert_eq!(conv.fx_rate_source, Some(FxRateSource::Series));
}

#[test]
fn convert_to_native_on_edit_requeries_when_currency_changed() {
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-usd", "USD", "CNY", "2026-01-05", 7.1);
    let baseline = reuse_baseline(
        1000,
        "HKD",
        "2026-01-07",
        NativeConversion {
            native_cents: 900,
            fx_rate_used: Some(0.9),
            fx_rate_source: Some(FxRateSource::Series),
        },
    );
    let conv =
        convert_to_native_on_edit(&conn, 1000, "USD", "2026-01-07", None, Some(&baseline)).unwrap();
    assert_eq!(conv.native_cents, 7100, "按新币种序列值重算");
    assert_eq!(conv.fx_rate_used, Some(7.1));
    assert_eq!(conv.fx_rate_source, Some(FxRateSource::Series));
}

#[test]
fn convert_to_native_on_edit_fails_coded_when_changed_and_week_missing() {
    let conn = test_support::open();
    test_support::seed_exchange_rate(&conn, "HKD", "CNY", 0.91);
    let baseline = reuse_baseline(
        1000,
        "HKD",
        "2026-01-07",
        NativeConversion {
            native_cents: 900,
            fx_rate_used: Some(0.9),
            fx_rate_source: Some(FxRateSource::Series),
        },
    );
    let err = convert_to_native_on_edit(&conn, 2000, "HKD", "2026-01-07", None, Some(&baseline))
        .unwrap_err();
    match err {
        AppError::Coded { code, params, .. } => {
            assert_eq!(code, "fx.rate-missing");
            assert_eq!(params, vec!["HKD".to_string(), "CNY".to_string()]);
        }
        other => panic!("应为既有码化缺失错误，实际: {other}"),
    }
}

#[test]
fn convert_to_native_on_edit_without_baseline_matches_trade_date_entry() {
    let conn = test_support::open();
    let conv = convert_to_native_on_edit(&conn, 12345, "CNY", "2026-01-07", None, None).unwrap();
    assert_eq!(conv.native_cents, 12345);
    assert_eq!(conv.fx_rate_used, None);
    assert_eq!(conv.fx_rate_source, None);

    test_support::seed_fx_rate_history(&conn, "fxh-new", "USD", "CNY", "2026-01-05", 7.5);
    let conv = convert_to_native_on_edit(&conn, 10000, "USD", "2026-01-07", None, None).unwrap();
    assert_eq!(conv.native_cents, 75000);
    assert_eq!(conv.fx_rate_used, Some(7.5));
    assert_eq!(conv.fx_rate_source, Some(FxRateSource::Series));
}

#[test]
fn convert_to_native_on_trade_date_does_not_fall_back_to_current_table() {
    let conn = test_support::open();
    test_support::seed_exchange_rate(&conn, "USD", "CNY", 7.2);
    assert!(convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", None).is_err());

    test_support::seed_fx_rate_history(&conn, "fxh-cur", "USD", "CNY", "2026-01-05", 7.5);
    assert_eq!(
        convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", None)
            .unwrap()
            .native_cents,
        75000
    );
}

#[test]
fn convert_to_native_on_trade_date_explicit_rate_skips_series_and_traces_explicit() {
    let conn = test_support::open();
    test_support::seed_fx_rate_history(&conn, "fxh-hit", "HKD", "CNY", "2026-01-05", 0.9);
    let conv = convert_to_native_on_trade_date(&conn, 52_508_160, "HKD", "2026-01-07", Some(0.88))
        .unwrap();
    assert_eq!(
        conv.native_cents, 46_207_181,
        "native = amount × 显式汇率（到分）"
    );
    assert_eq!(conv.fx_rate_used, Some(0.88), "留痕 = 显式给定值本身");
    assert_eq!(conv.fx_rate_source, Some(FxRateSource::Explicit));

    let conn = test_support::open();
    let conv =
        convert_to_native_on_trade_date(&conn, 10000, "USD", "2026-01-07", Some(7.19)).unwrap();
    assert_eq!(conv.native_cents, 71900);
    assert_eq!(conv.fx_rate_used, Some(7.19));
    assert_eq!(conv.fx_rate_source, Some(FxRateSource::Explicit));
}

#[test]
fn convert_to_native_on_trade_date_rejects_non_positive_explicit_rate() {
    for rate in [0.0, -0.85, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let conn = test_support::open();
        let err = convert_to_native_on_trade_date(&conn, 10000, "HKD", "2026-01-07", Some(rate))
            .unwrap_err();
        match err {
            AppError::Coded { code, params, .. } => {
                assert_eq!(code, "fx.explicit-rate-non-positive");
                assert_eq!(params, vec![rate.to_string()]);
            }
            other => panic!("应为码化错误，实际: {other}"),
        }
    }
}

#[test]
fn convert_to_native_on_trade_date_rejects_explicit_rate_on_same_currency_row() {
    let conn = test_support::open();
    let native = default_currency_code(&conn).unwrap();
    let err = convert_to_native_on_trade_date(&conn, 10000, &native, "2026-01-07", Some(1.0))
        .unwrap_err();
    match err {
        AppError::Coded { code, message, .. } => {
            assert_eq!(code, "fx.explicit-rate-direction-mismatch");
            assert!(message.contains("方向不符"), "实际: {message}");
        }
        other => panic!("应为码化错误，实际: {other}"),
    }
    let conv = convert_to_native_on_trade_date(&conn, 12345, &native, "2026-01-07", None).unwrap();
    assert_eq!(conv.native_cents, 12345);
    assert_eq!(conv.fx_rate_used, None);
    assert_eq!(conv.fx_rate_source, None);
}

#[test]
fn convert_to_native_on_edit_explicit_rate_beats_baseline_reuse() {
    let conn = test_support::open();
    let baseline = reuse_baseline(
        1000,
        "HKD",
        "2026-01-07",
        NativeConversion {
            native_cents: 900,
            fx_rate_used: Some(0.9),
            fx_rate_source: Some(FxRateSource::Series),
        },
    );
    let conv = convert_to_native_on_edit(
        &conn,
        1000,
        "HKD",
        "2026-01-07",
        Some(0.88),
        Some(&baseline),
    )
    .unwrap();
    assert_eq!(conv.native_cents, 880, "按显式给定值折算");
    assert_eq!(conv.fx_rate_used, Some(0.88));
    assert_eq!(
        conv.fx_rate_source,
        Some(FxRateSource::Explicit),
        "来源改标显式"
    );

    let err =
        convert_to_native_on_edit(&conn, 1000, "HKD", "2026-01-07", Some(0.0), Some(&baseline))
            .unwrap_err();
    assert_eq!(err.code(), Some("fx.explicit-rate-non-positive"));
}

#[test]
fn zero_cash_leg_is_zero_without_trace() {
    assert_eq!(
        NativeConversion::zero_cash_leg(),
        NativeConversion {
            native_cents: 0,
            fx_rate_used: None,
            fx_rate_source: None,
        }
    );
}
