//! 概览视图失效信号重读探针（lib 单测；headless 引擎内驱动——失效信号广播
//! 必须发生在引擎 UI 线程，集成测试线程触不到 thread_local 订阅者表，探针
//! 组件在引擎线程内经真实按钮触发广播，消费面挂**真实概览视图**）。
//!
//! 「删除即红」负向条目：删 [`crate::dashboard::DashboardView`] 内的三族
//! 订阅接线任一 → 本测红（库中已变而卡面仍旧）；删 [`crate::signals`] 的
//! 广播转发 → 探针按钮失去生产语义（广播不再到达订阅者）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use ledger_accounts::{AccountInput, AccountType};
use ledger_categories::CategoryInput;
use ledger_infra::ids::new_uuid;
use ledger_transaction::{TransactionInput, TransactionKind};
use nsh_core::prelude::*;
use nsh_engine::{EngineConfig, Environment, Idiom, Os, Root};
use nsh_testing::Headless;

use crate::app::{reset_book_assembly, run_startup, set_book_dir, test_assembly_guard};
use crate::dashboard;
use crate::signals;

/// 探针根组件：三族失效信号的广播按钮（引擎线程真实按压路径的等价形态，
/// 生产语义 = 各写路径成功后的广播接线）+ 真实概览视图。
#[component]
fn DashboardProbeApp() -> impl IntoElement {
    view()
        .flex_1()
        .flex_col()
        .app_class("bg-background")
        .child(
            view()
                .flex_row()
                .gap_2()
                .child(
                    view()
                        .test_id("probe-broadcast-transactions")
                        .on_press(move |_: &()| signals::notify_transactions_invalidated())
                        .child(text("交易失效")),
                )
                .child(
                    view()
                        .test_id("probe-broadcast-budgets")
                        .on_press(move |_: &()| signals::notify_budgets_invalidated())
                        .child(text("预算失效")),
                )
                .child(
                    view()
                        .test_id("probe-broadcast-investments")
                        .on_press(move |_: &()| signals::notify_investments_invalidated())
                        .child(text("投资失效")),
                ),
        )
        .child(dashboard::dashboard_view())
}

/// 概览全景账本（卡面读数与集成测试 seed_dashboard_book 同刻度，见
/// tests/app_headless.rs 同名助手注释）：净资产 ¥400、本月收支 500/200/300、
/// 投资三卡 ¥100/¥99/¥99、自由度 0.8%/积累期、预算卡 375/200。
fn seed_dashboard_book(tag: &str) -> std::path::PathBuf {
    use ledger_budget::{BudgetInput, BudgetPeriod};
    use ledger_investment::{InstrumentInput, InstrumentType, MarketPriceInput};

    let dir = std::env::temp_dir().join(format!("nsh-app-probe-{tag}-{}", new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ledger.db"), b"").unwrap();
    let db = crate::boot::boot_ledger(&dir).unwrap();
    let today = crate::boot::today_string();
    let create = |input: TransactionInput| {
        let conn = db.conn.lock().unwrap();
        ledger_transaction::create(&conn, input).unwrap();
    };
    let cash = {
        let conn = db.conn.lock().unwrap();
        ledger_accounts::create_account(
            &conn,
            AccountInput {
                name: "现金".to_owned(),
                kind: AccountType::Cash,
                currency_code: "CNY".to_owned(),
                initial_balance_cents: Some(0),
                credit_limit_cents: None,
                statement_day: None,
                due_day: None,
            },
        )
        .unwrap()
    };
    let food = {
        let conn = db.conn.lock().unwrap();
        ledger_categories::create_category(
            &conn,
            CategoryInput {
                name: "餐饮".to_owned(),
                kind: "expense".to_owned(),
                parent_id: None,
                icon: None,
            },
        )
        .unwrap()
    };
    {
        let conn = db.conn.lock().unwrap();
        ledger_budget::crud::create_budget(
            &conn,
            &BudgetInput {
                category_id: food,
                period: Some(BudgetPeriod::Monthly),
                amount_cents: 3_125,
                start_date: crate::boot::today_string(),
            },
        )
        .unwrap();
    }
    create(TransactionInput {
        kind: TransactionKind::Income,
        amount_cents: 50_000,
        currency_code: "CNY".to_owned(),
        account_id: Some(cash.clone()),
        date: today.clone(),
        note: Some("本月收入".to_owned()),
        ..probe_txn_input()
    });
    create(TransactionInput {
        kind: TransactionKind::Expense,
        amount_cents: 20_000,
        currency_code: "CNY".to_owned(),
        account_id: Some(cash.clone()),
        category_id: Some({
            let conn = db.conn.lock().unwrap();
            ledger_categories::list_categories(&conn, false)
                .unwrap()
                .into_iter()
                .find(|c| c.name == "餐饮")
                .map(|c| c.id)
                .unwrap()
        }),
        date: today.clone(),
        note: Some("本月支出".to_owned()),
        ..probe_txn_input()
    });
    let invest = {
        let conn = db.conn.lock().unwrap();
        ledger_accounts::create_account(
            &conn,
            AccountInput {
                name: "证券".to_owned(),
                kind: AccountType::Investment,
                currency_code: "CNY".to_owned(),
                initial_balance_cents: Some(100),
                credit_limit_cents: None,
                statement_day: None,
                due_day: None,
            },
        )
        .unwrap()
    };
    let instrument = {
        let conn = db.conn.lock().unwrap();
        let id = ledger_investment::create_instrument(
            &conn,
            InstrumentInput {
                symbol: "TST".to_owned(),
                kind: InstrumentType::Stock,
                name: Some("测试标的".to_owned()),
                currency_code: "CNY".to_owned(),
                market: None,
            },
        )
        .unwrap();
        ledger_investment::create_market_price(
            &conn,
            MarketPriceInput {
                instrument_id: id.clone(),
                price_cents: 1_000_000,
                currency_code: "CNY".to_owned(),
                priced_at: today.clone(),
                source: Some("manual".to_owned()),
            },
        )
        .unwrap();
        id
    };
    create(TransactionInput {
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "CNY".to_owned(),
        account_id: Some(invest),
        instrument_id: Some(instrument),
        quantity: Some(1.0),
        price_cents: Some(10_000),
        fee_cents: Some(0),
        date: today,
        note: Some("买入".to_owned()),
        ..probe_txn_input()
    });
    dir
}

/// 探针造数交易输入的最小基座（具名字段覆盖，其余零值）。
fn probe_txn_input() -> TransactionInput {
    TransactionInput {
        currency_code: "CNY".to_owned(),
        ..probe_txn_input_zero()
    }
}

fn probe_txn_input_zero() -> TransactionInput {
    TransactionInput {
        kind: TransactionKind::Expense,
        amount_cents: 0,
        currency_code: String::new(),
        account_id: None,
        to_account_id: None,
        funding_account_id: None,
        funding: Vec::new(),
        purchases: Vec::new(),
        category_id: None,
        merchant_id: None,
        merchant_name: None,
        policy_id: None,
        refund_of_transaction_id: None,
        note: None,
        source_order_no: None,
        date: String::new(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        origin: None,
        fx_rate: None,
        idempotency_key: None,
    }
}

fn probe_config() -> EngineConfig {
    EngineConfig {
        viewport: nsh_engine::Size::new(nsh_engine::Points(1024.0), nsh_engine::Points(720.0)),
        environment: Environment {
            os: Os::Linux,
            idiom: Idiom::Desktop,
            ..Environment::default()
        },
        ui_sets: vec![nsh_base::registry()],
        ..EngineConfig::new(Root::of::<DashboardProbeApp>())
    }
}

/// 失效信号重读（按卡片消费面分）：交易 → 净资产/本月收支（含预算卡的已支
/// 口径）；预算 → 自由度分母 + 预算卡；投资 → 投资卡 + 自由度分子 + 净资产
/// 持仓腿。删除概览视图任一订阅接线即红（模块注）。
#[test]
fn dashboard_cards_reread_on_invalidation_signals() {
    let _guard = test_assembly_guard();
    reset_book_assembly();
    let dir = seed_dashboard_book("dashboard-reread");
    set_book_dir(dir.clone());
    // 引导会话就位（探针根组件挂真实概览视图，读经 db_slot）。
    assert!(matches!(run_startup(), crate::app::StartupView::Ready));

    let mut app = Headless::start(probe_config()).unwrap();
    app.wait_for("id:net-worth-value").unwrap();
    app.wait_for("text:¥400").unwrap();

    // 交易失效：追加本月收入 10_000 → 净资产 ¥400→¥500、本月收入 500→600。
    {
        let db_session = crate::app::session_handle().unwrap();
        let conn = db_session.conn.lock().unwrap();
        let cash = ledger_accounts::list_accounts(&conn)
            .unwrap()
            .into_iter()
            .find(|a| a.name == "现金")
            .map(|a| a.id)
            .unwrap();
        ledger_transaction::create(
            &conn,
            TransactionInput {
                kind: TransactionKind::Income,
                amount_cents: 10_000,
                currency_code: "CNY".to_owned(),
                account_id: Some(cash),
                date: crate::boot::today_string(),
                note: Some("失效后收入".to_owned()),
                ..probe_txn_input()
            },
        )
        .unwrap();
    }
    app.press("id:probe-broadcast-transactions").unwrap();
    app.wait_for("text:¥500").unwrap();
    app.wait_for("text:600").unwrap();

    // 预算失效：追加月度预算 3_125 → 分母翻倍（年化总额 375→750：预算卡与
    // 自由度口径行随行；比例 = 10000×3%÷75000×100 = 0.4%）。
    {
        let db_session = crate::app::session_handle().unwrap();
        let conn = db_session.conn.lock().unwrap();
        let food = ledger_categories::list_categories(&conn, false)
            .unwrap()
            .into_iter()
            .find(|c| c.name == "餐饮")
            .map(|c| c.id)
            .unwrap();
        ledger_budget::crud::create_budget(
            &conn,
            &ledger_budget::BudgetInput {
                category_id: food,
                period: Some(ledger_budget::BudgetPeriod::Monthly),
                amount_cents: 3_125,
                start_date: crate::boot::today_string(),
            },
        )
        .unwrap();
    }
    app.press("id:probe-broadcast-budgets").unwrap();
    app.wait_for("text:750").unwrap();
    app.wait_for("text:0.4%").unwrap();

    // 投资失效：标的现价翻倍（持仓市值 10_000→20_000）→ 投资卡 ¥100→¥200、
    // 净资产 ¥500→¥600（持仓腿随行情）、自由度分子随行（口径行 ¥200）。
    {
        let db_session = crate::app::session_handle().unwrap();
        let conn = db_session.conn.lock().unwrap();
        let instrument = ledger_investment::list_instruments(
            &conn,
            &ledger_investment::InstrumentListFilter::default(),
        )
        .unwrap()
        .items
        .into_iter()
        .find(|i| i.symbol == "TST")
        .map(|i| i.id)
        .unwrap();
        ledger_investment::create_market_price(
            &conn,
            ledger_investment::MarketPriceInput {
                instrument_id: instrument,
                price_cents: 2_000_000,
                currency_code: "CNY".to_owned(),
                priced_at: crate::boot::today_string(),
                source: Some("manual".to_owned()),
            },
        )
        .unwrap();
    }
    app.press("id:probe-broadcast-investments").unwrap();
    app.wait_for("text:¥200").unwrap();
    app.wait_for("text:¥600").unwrap();
}
