#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

use ledger_investment::PnlFilter;
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::{TransactionInput, create_transaction_internal};
use tauri::Manager;
use tauri_app_lib::commands::investment::realized_pnl_summary;

use ledger_infra::db::{self, DbState};
use tauri_app_lib::test_support::{
    ScratchDir, seed_account, seed_fx_history_weeks, seed_instrument,
};

fn device_app(tag: &str) -> (tauri::App<tauri::test::MockRuntime>, ScratchDir) {
    ledger_accounts::balance::install_balance_refresh_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let dir = ScratchDir::new(&format!("pnl-it-{tag}"));
    let app = tauri::test::mock_app();
    app.manage(db::open_db_in(&dir).expect("文件库应可开"));
    (app, dir)
}

fn trade_input(
    kind: TransactionKind,
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price_cents: i64,
    fee_cents: i64,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        funding: Vec::new(),
        merchant_name: None,
        policy_id: None,
        kind,
        amount_cents: 0,
        currency_code: "USD".into(),
        account_id: Some(account_id.into()),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
        note: None,
        date: date.into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price_cents),
        fee_cents: Some(fee_cents),
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

fn dividend_input(
    account_id: &str,
    instrument_id: &str,
    amount_cents: i64,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        funding: Vec::new(),
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Dividend,
        amount_cents,
        currency_code: "USD".into(),
        account_id: Some(account_id.into()),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
        note: None,
        date: date.into(),
        instrument_id: Some(instrument_id.into()),
        quantity: None,
        price_cents: None,
        fee_cents: Some(0),
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

#[tokio::test]
async fn realized_pnl_summary_returns_single_value_rows_with_native_currency() {
    let (app, _dir) = device_app("contract");
    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().expect("种子写入锁应可取");
        seed_account(&guard, "acc-usd", "美股券商", "investment", "USD", 0);
        seed_instrument(&guard, "inst-x", "XX", "标的X", "USD", "nasdaq");
        seed_fx_history_weeks(&guard, "USD", "CNY", 7.0, &["2026-01-10", "2026-01-20"]);
        seed_fx_history_weeks(&guard, "USD", "CNY", 6.0, &["2026-02-01"]);
        create_transaction_internal(
            &guard,
            trade_input(
                TransactionKind::Buy,
                "acc-usd",
                "inst-x",
                10.0,
                1_000_000,
                0,
                "2026-01-10",
            ),
        )
        .expect("建仓应成功");
        create_transaction_internal(
            &guard,
            trade_input(
                TransactionKind::Sell,
                "acc-usd",
                "inst-x",
                5.0,
                1_200_000,
                200,
                "2026-01-20",
            ),
        )
        .expect("卖出应成功");
        create_transaction_internal(
            &guard,
            dividend_input("acc-usd", "inst-x", 100_000, "2026-02-01"),
        )
        .expect("分红应成功");
    }

    let summary = realized_pnl_summary(app.state::<DbState>(), None)
        .await
        .expect("盈亏汇总应返回");

    assert_eq!(summary.by_year.len(), 1);
    assert_eq!(summary.by_year[0].year, "2026");
    assert_eq!(summary.by_year[0].native_currency, "CNY");
    assert_eq!(summary.by_year[0].realized_pnl_cents, Some(68_600));
    assert_eq!(summary.by_year[0].dividend_cents, Some(600_000));
    assert_eq!(summary.by_year[0].realized_gain_cents, Some(668_600));
    assert_eq!(summary.by_account.len(), 1);
    assert_eq!(summary.by_account[0].account_id, "acc-usd");
    assert_eq!(summary.by_account[0].native_currency, "CNY");
    assert_eq!(summary.by_account[0].realized_gain_cents, Some(668_600));

    assert_eq!(
        serde_json::to_value(&summary).expect("应可序列化"),
        serde_json::json!({
            "by_year": [{
                "year": "2026",
                "native_currency": "CNY",
                "realized_pnl_cents": 68_600,
                "dividend_cents": 600_000,
                "realized_gain_cents": 668_600,
            }],
            "by_account": [{
                "account_id": "acc-usd",
                "account_name": "美股券商",
                "native_currency": "CNY",
                "realized_pnl_cents": 68_600,
                "dividend_cents": 600_000,
                "realized_gain_cents": 668_600,
            }],
        })
    );
}

#[tokio::test]
async fn realized_pnl_summary_filter_narrows_rows() {
    let (app, _dir) = device_app("filter");
    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().expect("种子写入锁应可取");
        seed_account(&guard, "acc-a", "账户A", "investment", "CNY", 0);
        seed_account(&guard, "acc-b", "账户B", "investment", "CNY", 0);
        seed_instrument(&guard, "inst-f", "FF", "标的F", "CNY", "sh");
        for acc in ["acc-a", "acc-b"] {
            create_transaction_internal(
                &guard,
                trade_input(
                    TransactionKind::Buy,
                    acc,
                    "inst-f",
                    10.0,
                    100_000,
                    0,
                    "2026-01-10",
                ),
            )
            .expect("建仓应成功");
            create_transaction_internal(
                &guard,
                trade_input(
                    TransactionKind::Sell,
                    acc,
                    "inst-f",
                    4.0,
                    150_000,
                    0,
                    "2026-01-20",
                ),
            )
            .expect("卖出应成功");
        }
    }

    let filter = PnlFilter {
        account_id: Some("acc-a".into()),
        instrument_id: None,
    };
    let summary = realized_pnl_summary(app.state::<DbState>(), Some(filter))
        .await
        .expect("盈亏汇总应返回");

    assert_eq!(summary.by_account.len(), 1);
    assert_eq!(summary.by_account[0].account_id, "acc-a");
    assert_eq!(summary.by_account[0].realized_gain_cents, Some(2_000));
}

#[tokio::test]
async fn realized_pnl_summary_missing_leg_serializes_null_row() {
    let (app, _dir) = device_app("missing");
    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().expect("种子写入锁应可取");
        seed_account(&guard, "acc-gap", "美股券商", "investment", "USD", 0);
        seed_instrument(&guard, "inst-g", "GG", "标的G", "USD", "nasdaq");
        seed_fx_history_weeks(
            &guard,
            "USD",
            "CNY",
            7.0,
            &["2025-06-10", "2026-01-10", "2026-01-20"],
        );
        create_transaction_internal(
            &guard,
            trade_input(
                TransactionKind::Buy,
                "acc-gap",
                "inst-g",
                10.0,
                1_000_000,
                0,
                "2026-01-10",
            ),
        )
        .expect("建仓应成功");
        create_transaction_internal(
            &guard,
            trade_input(
                TransactionKind::Sell,
                "acc-gap",
                "inst-g",
                5.0,
                1_200_000,
                200,
                "2026-01-20",
            ),
        )
        .expect("卖出应成功");
        guard
            .execute(
                "DELETE FROM fx_rate_history WHERE trade_date IN ('2026-01-05','2026-01-19')",
                [],
            )
            .expect("汇率历史删除应成功");
    }

    let summary = realized_pnl_summary(app.state::<DbState>(), None)
        .await
        .expect("盈亏汇总应返回（缺料行是值缺失不是错误）");

    assert_eq!(summary.by_year.len(), 1);
    assert_eq!(
        serde_json::to_value(&summary.by_year[0]).expect("应可序列化"),
        serde_json::json!({
            "year": "2026",
            "native_currency": "CNY",
            "realized_pnl_cents": null,
            "dividend_cents": null,
            "realized_gain_cents": null,
        })
    );
}

#[tokio::test]
async fn realized_pnl_summary_pair_without_history_raises_rate_missing() {
    let (app, _dir) = device_app("no-fx");
    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().expect("种子写入锁应可取");
        seed_account(&guard, "acc-zero", "美股券商", "investment", "USD", 0);
        seed_instrument(&guard, "inst-z", "ZZ", "标的Z", "USD", "nasdaq");
        seed_fx_history_weeks(&guard, "USD", "CNY", 7.0, &["2026-01-10", "2026-01-20"]);
        create_transaction_internal(
            &guard,
            trade_input(
                TransactionKind::Buy,
                "acc-zero",
                "inst-z",
                10.0,
                1_000_000,
                0,
                "2026-01-10",
            ),
        )
        .expect("建仓应成功");
        create_transaction_internal(
            &guard,
            trade_input(
                TransactionKind::Sell,
                "acc-zero",
                "inst-z",
                5.0,
                1_200_000,
                200,
                "2026-01-20",
            ),
        )
        .expect("卖出应成功");
        guard
            .execute("DELETE FROM fx_rate_history", [])
            .expect("汇率历史删除应成功");
    }

    let err = realized_pnl_summary(app.state::<DbState>(), None)
        .await
        .expect_err("币对零历史应报错");
    assert!(
        err.is_code("fx.rate-missing"),
        "币对零历史应报 fx.rate-missing，实际 {err:?}"
    );
}
