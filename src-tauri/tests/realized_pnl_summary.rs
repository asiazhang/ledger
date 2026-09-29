//! 盈亏汇总命令面集成测试（issue #1845 单值契约，ADR-0087 壳行为权威层）。
//!
//! 断言面 = 命令契约与返回值形状（对照 investment_overview 集成测试先例）：
//! 单值行形状（行标识 + 主值 + 两腿拆解 + native_currency）、线格式全量相等
//! （total / by_instrument 死字段随 #1845 删除，前端类型镜像此形状）、缺汇率的
//! 码化错误契约、缺料行的 null 形状。逐腿事件周折算口径的展开（兜底边界、
//! 同币种恒等、缺料不以零计入等）归域单测（`crates/investment/src/tests/pnl.rs`），
//! 多语句读闭包的快照探针亦住域层（`tests/read_snapshot.rs`），本层不重复。
//!
//! 造数纪律（ADR-0087 决策 3）：静态基线走统一测试数据库工厂，行为前置经公开
//! 写入口（交易写入）；「币对零历史」读场景需要库内状态直置（写路径必须先有
//! 卖出周汇率才能落库），DELETE 直写在测试内登记动机，先例：e2e 隐藏账户步骤。

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

use ledger_investment::PnlFilter;
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::{TransactionInput, create_transaction_internal};
use tauri::Manager;
use tauri_app_lib::commands::investment::realized_pnl_summary;

use ledger_infra::db::{self, DbState};
use tauri_app_lib::test_support::{
    ScratchDir, seed_account, seed_fx_history_weeks, seed_instrument,
};

/// mock 应用 + 独立临时目录文件库（`investment_overview` 集成测试同款现场：
/// 产品建连缝 + 交易域接缝接线，本位币读取钩子随此装入）。库目录走
/// ScratchDir（issue #1645）：guard 随元组交调用方持有，用例结束整棵删除。
fn device_app(tag: &str) -> (tauri::App<tauri::test::MockRuntime>, ScratchDir) {
    // 写路径副作用接线（余额缓存重算实现随此装入，未接线时写入即码化拒绝）。
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

/// 现金分红输入（现金腿 = `amount_cents`，币种须与到账账户一致）。
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

/// 契约：单值行形状 + native_currency 透出 + 逐腿事件周折算在命令面出数；
/// 线格式全量相等（前端 `RealizedPnlSummary` 类型镜像此字段集）——
/// `total` / `by_instrument` 死字段已随 #1845 删除，混入即红。
#[tokio::test]
async fn realized_pnl_summary_returns_single_value_rows_with_native_currency() {
    let (app, _dir) = device_app("contract");
    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().expect("种子写入锁应可取");
        seed_account(&guard, "acc-usd", "美股券商", "investment", "USD", 0);
        seed_instrument(&guard, "inst-x", "XX", "标的X", "USD", "nasdaq");
        // 事件周汇率：买入周 01-05 与卖出周 01-19 → 7.0；分红到账周 01-26 → 6.0。
        seed_fx_history_weeks(&guard, "USD", "CNY", 7.0, &["2026-01-10", "2026-01-20"]);
        seed_fx_history_weeks(&guard, "USD", "CNY", 6.0, &["2026-02-01"]);
        // 买入 10@100 元、卖出 5@120 元 fee 2 元 → +9800 分 ×7 = 68600；
        // 分红 1000 元 → 100000 分 ×6 = 600000。
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

    // 主值与两腿拆解（单值行契约，域内折算、前端零算术）。
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

    // 线格式契约：字段集恰为 by_year / by_account（total / by_instrument 已删除）。
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

/// 筛选参数贯通命令面：账户筛选收窄行集（壳三件套的参数解包断言面）。
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

/// 缺料行的 null 形状：兜底窗口内无汇率点 → 三项全 null（前端渲染「无法计算」，
/// 不给半截数字——线格式显式表达缺料，而非 0）。
#[tokio::test]
async fn realized_pnl_summary_missing_leg_serializes_null_row() {
    let (app, _dir) = device_app("missing");
    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().expect("种子写入锁应可取");
        seed_account(&guard, "acc-gap", "美股券商", "investment", "USD", 0);
        seed_instrument(&guard, "inst-g", "GG", "标的G", "USD", "nasdaq");
        // 写入需要卖出周汇率（写路径纪律），先种后删：删后唯一历史点远在窗口外。
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
        // 读场景直置（登记动机见文件头）：删去事件周与就近窗口内全部点。
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
    // 线格式 null 形状：三项全 null（契约面不给半截数字，不以零计入）。
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

/// 错误契约：币对零历史（正反向均无行）→ `fx.rate-missing` 码化错误指路同步。
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
        // 读场景直置（登记动机见文件头）：清空币对全部历史 → 命令面错误路径。
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
