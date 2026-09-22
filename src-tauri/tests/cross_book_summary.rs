//! 跨账本投资汇总命令面集成测试（issue #1196 / ADR-0114）。
//!
//! 独立测试二进制（与 `tests/commands/` 分进程）：汇总编排是唯一同时消费注册表
//! 清单与多本库文件的命令面，`$HOME` 隔离与注册表现场必须独占——`tests/commands/`
//! 二进制内 books 旅程已共用同一进程级 HOME 现场（见其 isolation.rs），本文件
//! 自持一份进程内 `Once` 隔离，两二进制互不可见、并行安全。
//!
//! 覆盖壳编排整链（单顺序旅程，场景间有前后依赖）：注册表未就绪的码化拒绝 →
//! 多本登记与四态逐本分派（明文/空库/密文/版本不一致）→ 缺当期汇率错误上抛 →
//! 补汇率后的完整合计。只读承诺以「调用前后库文件字节不变」钉住（ADR-0114：
//! 读路径聚合、零新增持久状态）。

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

use std::path::Path;
use std::sync::Once;

use ledger_infra::db::data_location::{self, DB_FILE_NAME};
use ledger_infra::db::encryption::enable_encryption_for_file;
use ledger_infra::db::{open_connection_in, open_connection_readonly_in, schema_version};
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::{TransactionInput, create_transaction_internal};
use tauri::Manager;
use tauri_app_lib::commands::book;
use tauri_app_lib::commands::boot::BootCell;
use tauri_app_lib::commands::cross_book_summary::cross_book_investment_summary;
use tauri_app_lib::cross_book_summary::{CrossBookBookStatus, CrossBookInvestmentSummary};
use tauri_app_lib::test_support::{
    ScratchDir, seed_account, seed_exchange_rate, seed_instrument, seed_market_price,
};

/// HOME 重定向（本二进制进程内一次；理由与形态见 tests/commands/isolation.rs）。
fn isolate_home() {
    static ISOLATE: Once = Once::new();
    ISOLATE.call_once(|| {
        let root = std::env::temp_dir().join(format!(
            "ledger-crossbook-it-{}",
            ledger_infra::db::new_uuid()
        ));
        std::fs::create_dir_all(&root).unwrap();
        // SAFETY：`set_var` 自 Rust 2024 起 unsafe；本函数是本测试二进制唯一的
        // `$HOME` 写入点，`Once` 保证进程内至多执行一次，写入后该值不再变化。
        unsafe { std::env::set_var("HOME", &root) };
    });
}

/// mock 应用 + 独立临时目录文件库 + 真实引导登记态（sync_channel::device_app 同款：
/// 产品建缝拿连接，落库前显式注册写路径副作用实现）。库目录走 ScratchDir
/// （issue #1645）：guard 随元组交调用方持有，用例结束（含 panic）整棵删除。
fn device_app(tag: &str) -> (tauri::AppHandle<tauri::test::MockRuntime>, ScratchDir) {
    ledger_accounts::balance::install_balance_refresh_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let dir = ScratchDir::new(&format!("crossbook-it-{tag}"));
    let app = tauri::test::mock_app();
    let boot = data_location::boot(&dir);
    app.manage(BootCell::new(boot));
    app.manage(ledger_infra::db::open_db_in(&dir).unwrap());
    (app.handle().clone(), dir)
}

/// 买入输入构造器（活动本持仓腿；金额由 prepare 按数量×单价重算）。
fn buy_input(account_id: &str, instrument_id: &str) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-10".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(10.0),
        price_cents: Some(1_000_000),
        fee_cents: Some(0),
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
        origin: None,
        fx_rate: None,
    }
}

fn refresh_balances(conn: &rusqlite::Connection) {
    ledger_accounts::balance::refresh_all_account_balances(conn).unwrap();
}

fn file_bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}

fn status_of<'a>(summary: &'a CrossBookInvestmentSummary, name: &str) -> &'a CrossBookBookStatus {
    &summary
        .books
        .iter()
        .find(|b| b.name == name)
        .unwrap_or_else(|| panic!("汇总应含账本 {name}"))
        .status
}

#[tokio::test]
async fn cross_book_summary_command_surface_journey() {
    isolate_home();
    let (handle, dir) = device_app("journey");

    // -----------------------------------------------------------------
    // 场景〇：注册表未就绪（尚无注册表文件）→ 码化拒绝，不静默按单本出数。
    // -----------------------------------------------------------------
    let err = cross_book_investment_summary(handle.clone(), handle.state())
        .await
        .unwrap_err();
    assert!(
        err.is_code("book.registry-unavailable"),
        "注册表不可用应码化拒绝，实际 {err:?}"
    );

    // -----------------------------------------------------------------
    // 活动本（登记序首本，CNY）：投资账户现金 100_000 分 + 一笔持仓
    // （买 10 @ 100 元，现价 120 元 → 市值 120_000 分、未实现 20_000 分；
    // 买入直扣投资账户 → 现金归零，可投资资产 = 120_000 分）。
    // -----------------------------------------------------------------
    {
        let conn = open_connection_in(&dir).unwrap();
        seed_account(&conn, "acc-inv", "投资账户", "investment", "CNY", 100_000);
        seed_instrument(&conn, "inst-a", "IA", "标的A", "CNY", "sh");
        seed_exchange_rate(&conn, "CNY", "CNY", 1.0);
        create_transaction_internal(&conn, buy_input("acc-inv", "inst-a")).unwrap();
        seed_market_price(&conn, "inst-a", 1_200_000, "CNY");
        refresh_balances(&conn);
    }

    // 活动本 schema 版本（版本不一致场景的基准；活动本建连必经迁移）。
    let active_version = {
        let conn = open_connection_readonly_in(&dir).unwrap();
        schema_version(&conn).unwrap()
    };

    // -----------------------------------------------------------------
    // 登记四本非活动账本，各落一种逐本状态：空账本=未建库、美股账本=明文 USD、
    // 密码本=加密库、旧版本=user_version 落后一代。
    // -----------------------------------------------------------------
    let empty = book::create_book(handle.clone(), "空账本".into())
        .await
        .unwrap();
    let usd = book::create_book(handle.clone(), "美股账本".into())
        .await
        .unwrap();
    let locked = book::create_book(handle.clone(), "密码本".into())
        .await
        .unwrap();
    let stale = book::create_book(handle.clone(), "旧版本".into())
        .await
        .unwrap();

    // 明文 USD 本：投资账户现金 10_000 分；本位币基准设为 USD（app_settings 键值
    // 为 JSON 编码，settings::set 同款形状）——本内折算 USD→USD 无需汇率，
    // 跨本折算则落在活动本汇率表（被测路径）。
    {
        let conn = open_connection_in(&usd.dir).unwrap();
        seed_account(&conn, "acc-usd", "美股现金", "investment", "USD", 10_000);
        conn.execute(
            "INSERT INTO app_settings(key, value) VALUES('ledger.base_currency', '\"USD\"') \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
        )
        .unwrap();
        refresh_balances(&conn);
    }
    // 加密本：建真库后转密文（books.rs 旅程同款接缝）。
    {
        let conn = open_connection_in(&locked.dir).unwrap();
        drop(conn);
        enable_encryption_for_file(&locked.dir.join(DB_FILE_NAME), "pw-1234").unwrap();
    }
    // 旧版本本：建真库后把 user_version 拨回一代，模拟未随应用升级的存量本。
    {
        let conn = open_connection_in(&stale.dir).unwrap();
        drop(conn);
        let raw = rusqlite::Connection::open(stale.dir.join(DB_FILE_NAME)).unwrap();
        raw.pragma_update(None, "user_version", active_version - 1)
            .unwrap();
    }

    // -----------------------------------------------------------------
    // 场景一：缺当期汇率（活动本汇率表无 USD→CNY）→ 折算腿错误上抛，
    // 不静默返回缺料合计（与本内折算同一口径）。
    // -----------------------------------------------------------------
    let err = cross_book_investment_summary(handle.clone(), handle.state())
        .await
        .unwrap_err();
    assert!(
        err.is_code("fx.rate-missing"),
        "缺汇率应报 fx.rate-missing，实际 {err:?}"
    );

    // 活动本汇率表补 USD→CNY（跨本折算取活动本当期汇率表，ADR-0114 决策 3）。
    {
        let conn = open_connection_in(&dir).unwrap();
        seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    }

    // 只读承诺基线：调用前快照五本库文件字节。
    let mut before = std::collections::BTreeMap::new();
    for (name, book_dir) in [
        ("active", dir.path().to_path_buf()),
        ("empty", empty.dir.clone()),
        ("usd", usd.dir.clone()),
        ("locked", locked.dir.clone()),
        ("stale", stale.dir.clone()),
    ] {
        before.insert(name, file_bytes(&book_dir.join(DB_FILE_NAME)));
    }

    let summary = cross_book_investment_summary(handle.clone(), handle.state())
        .await
        .expect("汇总应成功");

    // 只读承诺：调用后五本库文件字节不变（无迁移、无任何写入）。
    for (name, book_dir) in [
        ("active", dir.path()),
        ("empty", empty.dir.as_path()),
        ("usd", usd.dir.as_path()),
        ("locked", locked.dir.as_path()),
        ("stale", stale.dir.as_path()),
    ] {
        assert_eq!(
            before.get(name).unwrap(),
            &file_bytes(&book_dir.join(DB_FILE_NAME)),
            "账本 {name} 的库文件应零变化"
        );
    }

    // 折算口径：目标＝活动本本位币，且发生过折算（USD 本计入）。
    assert_eq!(summary.target_currency, "CNY");
    assert!(summary.converted);

    // 逐本状态：注册表序、四类排除各自命中。
    assert_eq!(summary.books.len(), 5);
    assert_eq!(
        status_of(&summary, "默认账本"),
        &CrossBookBookStatus::Included
    );
    assert_eq!(
        status_of(&summary, "空账本"),
        &CrossBookBookStatus::NotInitialized
    );
    assert_eq!(
        status_of(&summary, "美股账本"),
        &CrossBookBookStatus::Included
    );
    assert_eq!(status_of(&summary, "密码本"), &CrossBookBookStatus::Locked);
    assert_eq!(
        status_of(&summary, "旧版本"),
        &CrossBookBookStatus::SchemaMismatch
    );

    // 合计（活动本 + 明文 USD 本；其余三本排除）：
    // 市值 120_000（活动本持仓）；未实现 20_000；累计收益 20_000；
    // 可投资资产 = 活动本 120_000 + USD 本 10_000×7 = 190_000。
    assert_eq!(summary.market_value_cents, 120_000);
    assert_eq!(summary.unrealized_pnl_cents, 20_000);
    assert_eq!(summary.cumulative_pnl_cents, 20_000);
    assert_eq!(summary.investable_assets_cents, 190_000);
}
