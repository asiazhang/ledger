//! **首次登记**引导路径端到端（ADR-0125 决策 1/4，issue #1410）：进程启动没有
//! 已登记的 `DbState`，引导序列自建连接对、登记状态、安装**进程级**门面，随后
//! 命令面照常读写——这条路径是生产启动路径，其余测试世界都直接管理
//! `DbState`（惰性门面路径），故单列本文件覆盖它。
//!
//! **接线证明的守门形态**（ADR-0087：接线型判据的断言对准用户可观察结果）：
//! 本测试对准「引导后建户 → 列户读回」这一用户可观察结果；而「首次登记分支确实
//! 安装进程级门面」是调用点形状（进程级门面的可观察差异只在故障恢复态，healthy
//! 路径下惰性门面行为等价），故由源码扫描守门核对——见根包
//! `db_slot_guard::first_registration_installs_process_level_facade`（删除
//! `install_facade` 调用即红，ADR-0087 允许接线以源码扫描守门替代）。
//!
//! **独立测试二进制**（`readonly_connection` / `sync_trigger_poll` 同款理由）：
//! 首次登记路径的引导落到就绪相位会经唯一编排点拉起后台服务（进程级单次拉起
//! 守卫），与 `tests/commands` 内刻意注入时机的同步触发接线测试在同一进程内
//! 互斥；本文件独立成进程，且用 mock 应用的 `app_data_dir()` 作库目录。

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

use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::encryption::EncryptionGate;
use tauri_app_lib::commands::accounts::{create_account, list_accounts};
use tauri_app_lib::commands::boot::restart_app;

type AppHandle = tauri::AppHandle<tauri::test::MockRuntime>;

/// `$HOME` 现场隔离（`readonly_connection` 同型）：mock runtime 的 `app_data_dir()`
/// 由 `$HOME` 派生，重定向后默认数据目录落在进程专属临时区。
fn isolate_home() {
    static ISOLATE: std::sync::Once = std::sync::Once::new();
    ISOLATE.call_once(|| {
        let root = std::env::temp_dir().join(format!(
            "ledger-facade-wiring-{}",
            ledger_infra::db::new_uuid()
        ));
        std::fs::create_dir_all(&root).unwrap();
        // SAFETY：set_var 自 Rust 2024 起 unsafe；本函数是本测试二进制唯一的
        // `$HOME` 写入点，Once 保证至多执行一次、写入后不再变化。
        unsafe { std::env::set_var("HOME", &root) };
    });
}

/// 无 `DbState` 的引导登记态夹具（首次登记路径的充分前置）。
fn fresh_app() -> AppHandle {
    let app = tauri::test::mock_app();
    let dir = app
        .path()
        .app_data_dir()
        .expect("mock app 应可解析 app_data_dir");
    std::fs::create_dir_all(&dir).unwrap();
    // 写路径副作用与交易域接缝接线（与测试工厂/生产启动同形，幂等）。
    ledger_accounts::balance::install_balance_refresh_hook();
    tauri_app_lib::transaction_wiring::install_all();
    app.manage(tauri_app_lib::commands::boot::BootCell::new(
        ledger_infra::db::data_location::boot(&dir),
    ));
    app.manage(EncryptionGate::new(false));
    app.manage(BootFailureGate::new());
    app.handle().clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn first_registration_boot_serves_commands_end_to_end() {
    isolate_home();
    let app = fresh_app();
    // 首次登记路径：本进程内尚无 DbState，引导序列建库（空明文库）并登记。
    restart_app(app.clone()).await.expect("首次登记引导应成功");

    // 门面在位后命令面照常可用（读侧与写侧由同一份门面服务）。
    let id = create_account(
        app.state(),
        app.clone(),
        ledger_accounts::AccountInput {
            name: "门面接线账户".into(),
            kind: ledger_accounts::AccountType::Cash,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(12_345),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .await
    .expect("写命令应成功");
    let rows = list_accounts(app.state()).await.expect("读命令应成功");
    assert!(
        rows.iter().any(|a| a.id == id && a.name == "门面接线账户"),
        "读命令应读到写命令落库的账户"
    );
}
