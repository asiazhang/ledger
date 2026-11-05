//! headless 组件测试（issue #2017；docs/research/nsh-testing-capabilities.md
//! §2.1 裁决的 UI 自动化接缝首件前置——壳骨架的窗口呈现接线）。
//!
//! 进程内起真实视图树（`Root::of::<App>`，无窗口无平台后端），断言只写用户
//! 可见结果：立桩靶标「交易总数：N 条」由真实组件树呈现。装配入口注入测试库
//! 路径（生产与测试同一装配路径，§3.1 lib + bin 双靶）；建库走产品开库入口、
//! 造数经域层公开写入口（与 lib 单测同纪律）。域栈引导在组件任务的后台线程
//! （blocking），呈现晚于步骤返回——一律 `wait_for`（§3.5 通用纪律 4）。

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

use std::fs;
use std::path::Path;

use ledger_accounts::{AccountInput, AccountType};
use ledger_infra::ids::new_uuid;
use ledger_transaction::{TransactionInput, TransactionKind};
// 暂存目录落 `ledger-test-` 前缀闭集（test_support/scratch.rs 头注）；本 crate
// 不消费根包工厂，前缀纪律自守。

use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

/// 立桩验收（issue #2017 AC1）：窗口显示真实账本库的交易总数（读路径直调域
/// 服务，无 IPC 边界）。删掉接线（装配注入 / 引导 / 读路径任一环）即红。
#[test]
fn app_window_shows_transaction_total_of_seeded_book() {
    // 测试账本：产品开库入口建库（0 字节文件 = 新装语义），域公开写入口造数。
    let dir = std::env::temp_dir().join(format!("ledger-test-nsh-app-headless-{}", new_uuid()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("ledger.db"), b"").unwrap();
    seed_one_expense(&dir);

    // 装配入口注入 → 起真实视图树 → 等待立桩靶标文本（后台引导结果）。
    nsh_app::app::set_book_dir(dir.clone());
    let mut app = Headless::start(EngineConfig::new(Root::of::<nsh_app::app::App>())).unwrap();
    app.wait_for("text:交易总数：1 条").unwrap();

    // 次行回显账本目录（窗口呈现可见凭据）。
    let dir_text = dir.display().to_string();
    assert!(
        app.screen().texts().iter().any(|t| *t == dir_text),
        "次行应回显账本目录：{:?}",
        app.screen().texts()
    );
}

/// 经域层公开写入口造一笔现金账户支出（造数纪律：CONTEXT-testing
/// 「公开写入口（测试侧）」，与 lib 单测同源同形）。
fn seed_one_expense(dir: &Path) {
    let db = nsh_app::boot::boot_ledger(dir).unwrap();
    let conn = db
        .conn
        .lock()
        .map_err(|e| ledger_infra::error::AppError::Db(e.to_string()))
        .unwrap();
    let account = ledger_accounts::create_account(
        &conn,
        AccountInput {
            name: "测试现金".to_owned(),
            kind: AccountType::Cash,
            currency_code: "CNY".to_owned(),
            initial_balance_cents: Some(0),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .unwrap();
    ledger_transaction::create(
        &conn,
        TransactionInput {
            kind: TransactionKind::Expense,
            amount_cents: 2_500,
            currency_code: "CNY".to_owned(),
            account_id: Some(account),
            to_account_id: None,
            funding_account_id: None,
            funding: Vec::new(),
            purchases: Vec::new(),
            category_id: None,
            merchant_id: None,
            merchant_name: None,
            policy_id: None,
            refund_of_transaction_id: None,
            note: Some("立桩造数".to_owned()),
            source_order_no: None,
            date: "2026-01-15".to_owned(),
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
        },
    )
    .unwrap();
}
