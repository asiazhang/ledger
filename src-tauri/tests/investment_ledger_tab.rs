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

use tauri::Manager;

use ledger_infra::db::{self, DbState};
use ledger_investment::InvestmentTransactionListFilter;
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::{TransactionInput, create_transaction_internal};
use tauri_app_lib::commands::investment::list_investment_transactions;
use tauri_app_lib::test_support::{ScratchDir, seed_account, seed_instrument};

fn device_app(
    tag: &str,
) -> (
    tauri::App<tauri::test::MockRuntime>,
    tauri_app_lib::test_support::ScratchDir,
) {
    ledger_accounts::balance::install_balance_refresh_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let dir = ScratchDir::new(&format!("ledger-tab-it-{tag}"));
    let app = tauri::test::mock_app();
    app.manage(db::open_db_in(&dir).expect("文件库应可开"));
    (app, dir)
}

fn buy_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price_cents: i64,
    fee_cents: i64,
) -> TransactionInput {
    TransactionInput {
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: Some(account_id.into()),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price_cents),
        fee_cents: Some(fee_cents),
        date: "2026-01-10".into(),
        ..plain_input()
    }
}

fn dividend_input(account_id: &str, instrument_id: &str, amount_cents: i64) -> TransactionInput {
    TransactionInput {
        kind: TransactionKind::Dividend,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: Some(account_id.into()),
        instrument_id: Some(instrument_id.into()),
        date: "2026-02-10".into(),
        ..plain_input()
    }
}

fn plain_input() -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Expense,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: Some(String::new()),
        funding: Vec::new(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
        note: None,
        date: "2026-01-01".into(),
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

#[tokio::test]
async fn list_investment_transactions_returns_created_buy_with_projection() {
    let (app, _dir) = device_app("one-step");
    let conn = app.state::<DbState>().conn.clone();
    let buy_id = {
        let guard = conn.lock().expect("种子写入锁应可取");
        seed_account(&guard, "acc-it", "券商户", "investment", "CNY", 0);
        seed_instrument(&guard, "inst-a", "AAPL", "苹果", "CNY", "unknown");
        create_transaction_internal(&guard, buy_input("acc-it", "inst-a", 10.0, 100_000, 100))
            .expect("建仓应成功")
            .id
    };

    let result = list_investment_transactions(app.state::<DbState>(), None)
        .await
        .expect("投资明细列表应返回");
    assert_eq!(result.total, 1);
    assert_eq!(result.items.len(), 1);
    let row = &result.items[0];
    assert_eq!(row.id, buy_id, "读回的行即刚创建的买入");
    assert_eq!(row.kind, TransactionKind::Buy);
    assert_eq!(row.account_id, "acc-it");
    assert_eq!(row.funding_account_id, None);
    assert_eq!(row.instrument_id, "inst-a");
    assert_eq!(row.symbol, "AAPL");
    let trade = row.trade.as_ref().expect("buy 行应带买卖载荷");
    assert_eq!(trade.quantity, 10.0);
    assert_eq!(trade.price_cents, 100_000);
    assert_eq!(trade.fee_cents, 100);

    assert_eq!(
        serde_json::to_value(&result.items).expect("应可序列化"),
        serde_json::json!([{
            "id": buy_id,
            "date": "2026-01-10",
            "kind": "buy",
            "amount_cents": 10_100,
            "account_id": "acc-it",
            "funding_account_id": null,
            "instrument_id": "inst-a",
            "symbol": "AAPL",
            "instrument_name": "苹果",
            "instrument_type": "stock",
            "trade": {"quantity": 10.0, "price_cents": 100_000, "fee_cents": 100},
            "convert": null,
            "split": null,
            "note": null,
            "currency_code": "CNY",
            "amount_native_cents": 10_100,
            "source_order_no": null,
        }])
    );
}

#[tokio::test]
async fn list_investment_transactions_unpacks_filter_dimensions() {
    let (app, _dir) = device_app("unpack");
    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().expect("种子写入锁应可取");
        seed_account(&guard, "acc-it", "券商户", "investment", "CNY", 0);
        seed_account(&guard, "acc-bank", "银行卡", "bank", "CNY", 0);
        seed_instrument(&guard, "inst-a", "AAPL", "苹果", "CNY", "unknown");
        create_transaction_internal(
            &guard,
            TransactionInput {
                funding_account_id: Some("acc-bank".into()),
                ..buy_input("acc-it", "inst-a", 1.0, 100_000, 0)
            },
        )
        .expect("建仓应成功");
        create_transaction_internal(&guard, dividend_input("acc-bank", "inst-a", 30_000))
            .expect("分红应成功");
    }

    let all = list_investment_transactions(app.state::<DbState>(), None)
        .await
        .expect("缺省过滤应返回全部");
    assert_eq!(all.total, 2);

    let by_account = list_investment_transactions(
        app.state::<DbState>(),
        Some(InvestmentTransactionListFilter {
            account_id: Some("acc-bank".into()),
            ..Default::default()
        }),
    )
    .await
    .expect("账户过滤应返回");
    assert_eq!(by_account.total, 2, "出资端 ∪ 到账端双命中");

    let dividends = list_investment_transactions(
        app.state::<DbState>(),
        Some(InvestmentTransactionListFilter {
            kinds: Some(vec![TransactionKind::Dividend]),
            ..Default::default()
        }),
    )
    .await
    .expect("kind 子集应返回");
    assert_eq!(dividends.total, 1);
    assert_eq!(dividends.items[0].kind, TransactionKind::Dividend);
    assert_eq!(dividends.items[0].amount_cents, 30_000, "现金腿金额");
}

#[tokio::test]
async fn list_investment_transactions_filter_rejects_unknown_kind_on_wire() {
    let result = serde_json::from_value::<InvestmentTransactionListFilter>(serde_json::json!({
        "kinds": ["bogus"]
    }));
    assert!(result.is_err(), "未知 kind 应在 IPC 反序列化边界拒收");
}
