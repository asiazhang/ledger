//! 投资明细列表命令面集成测试（ADR-0135 / issue #1778，ADR-0087 壳行为权威层）。
//!
//! 断言面 = 壳三件套与接线证明：参数解包（filter 缺省与四维载荷原样到达域层）、
//! wire 错误契约（filter 的 kind 闭集枚举拒未知值，不静默吞）、一步可观察
//! （创建买入 → 命令可读回该行，含行投影与线格式契约）。过滤/排序/分页/软删/
//! 快照语义的展开归域单测（`crates/investment/src/tests/ledger_tab.rs`），本层
//! 不重复其全部场景。
//!
//! 造数纪律（ADR-0087 决策 3）：静态基线走统一测试数据库工厂，行为前置经公开
//! 写入口（`create_transaction_internal`）。

// 测试整体豁免（ADR-0060）：集成测试 crate 经 cfg(test) 放行六件套，生产构建零放宽。
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

/// mock 应用 + 独立临时目录文件库（`investment_overview` 同款现场：产品建连缝 +
/// 交易域接缝接线）。库目录走 ScratchDir（issue #1645）：guard 随元组交调用方持有。
fn device_app(
    tag: &str,
) -> (
    tauri::App<tauri::test::MockRuntime>,
    tauri_app_lib::test_support::ScratchDir,
) {
    // 写路径副作用接线（与 investment_overview 现场同款：交易写入的余额缓存
    // 重算实现随此装入；未接线时写入即码化拒绝）。
    ledger_accounts::balance::install_balance_refresh_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let dir = ScratchDir::new(&format!("ledger-tab-it-{tag}"));
    let app = tauri::test::mock_app();
    app.manage(db::open_db_in(&dir).expect("文件库应可开"));
    (app, dir)
}

/// 买入输入（公开写入口造数）：数量 / 单价 / 手续费为热点，日期固定工厂日。
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
        account_id: account_id.into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price_cents),
        fee_cents: Some(fee_cents),
        date: "2026-01-10".into(),
        ..plain_input()
    }
}

/// 现金分红输入（到账账户可为任意在用账户，ADR-0109）。
fn dividend_input(account_id: &str, instrument_id: &str, amount_cents: i64) -> TransactionInput {
    TransactionInput {
        kind: TransactionKind::Dividend,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        instrument_id: Some(instrument_id.into()),
        date: "2026-02-10".into(),
        ..plain_input()
    }
}

/// 中性底座（其余字段缺省）。
fn plain_input() -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Expense,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: String::new(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
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
    }
}

/// 一步可观察（接线证明）：创建买入 → 命令读回该行——行投影（公共字段 + 买卖
/// 载荷）与线格式契约一并锁定（前端 `InvestmentTransactionRow` 类型镜像此形状）。
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

    // 线格式契约（前端 `InvestmentTransactionRow` 类型镜像此字段集）：改动即须同步前端。
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
        }])
    );
}

/// 参数解包：filter 缺省（None）= 不过滤返回全部；四维载荷经命令签名原样到达
/// 域层——账户（出资端 ∪ 到账端）与 kind 子集在本例逐一可见（口径展开归域单测）。
#[tokio::test]
async fn list_investment_transactions_unpacks_filter_dimensions() {
    let (app, _dir) = device_app("unpack");
    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().expect("种子写入锁应可取");
        seed_account(&guard, "acc-it", "券商户", "investment", "CNY", 0);
        seed_account(&guard, "acc-bank", "银行卡", "bank", "CNY", 0);
        seed_instrument(&guard, "inst-a", "AAPL", "苹果", "CNY", "unknown");
        // 带出资账户的买入（出资端命中前提）+ 到账银行卡的分红（到账端命中前提）。
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

    // 账户维：出资账户命中 buy（出资端）∪ dividend（到账端）。
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

    // 类型维：kind 子集多选。
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

/// wire 错误契约：filter 的 kind 是闭集枚举，未知值在 IPC 边界反序列化即报错，
/// 不静默吞掉变宽（与 HTTP 侧 `kinds` 逐元素闭集校验同纪律）。
#[tokio::test]
async fn list_investment_transactions_filter_rejects_unknown_kind_on_wire() {
    let err = serde_json::from_value::<InvestmentTransactionListFilter>(serde_json::json!({
        "kinds": ["bogus"]
    }))
    .expect_err("未知 kind 应反序列化报错");
    assert!(
        err.to_string().contains("交易类型"),
        "错误文案应来自交易类型闭集解析，实际 {err:?}"
    );
}
