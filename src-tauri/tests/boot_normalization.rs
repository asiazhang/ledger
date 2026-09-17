//! 启动归一化外来形态明文库的壳接线集成测试（issue #1453 / ADR-0087 接线证明）。
//!
//! 现场 = mock 应用 + 默认数据目录（`app_data_dir`，`restart_app` 原位重引导
//! 按同一路径解析）里一枚**外来形态**明文库：保留字节 12 的合法一页库
//!（`ledger_infra::test_utils` 按字节构造后交给 SQLite 自身写表写行，与基础设施
//! 单测同一份夹具——两份实现会漂移出「单测绿、集成测试另一种库」的假覆盖）。
//!
//! 断言对准**用户可观察结果**：库文件被归一化（偏移 20 = 0，夹具数据仍在）且壳
//! 启动相位落到 `ready`。负向判据（ADR-0087）：删掉 [`boot_sequence`] 里的启动
//! 归一化接线，明文建连照样成功、相位照样 `ready`，但文件偏移 20 仍是 12——本
//! 文件的偏移断言随之变红，故这条接线不会被静默移除。
//!
//! **独立测试二进制**（`readonly_connection` 同款理由）：本场景经 `restart_app`
//! 落到 `Ready` 会拉起后台服务（`start_background_services`），调度线程的单次拉起
//! 守卫是进程级单例——与 `tests/commands` 目标内刻意注入时机的 sync_trigger 接线
//! 测试在同一进程内互斥（先行的拉起会让其注入失效），故不落在 `tests/commands`。
//!
//! 三层权威（ADR-0087）：探测与归一化的行为权威在基础设施单测
//!（`infra::boot::tests`，含「归一化后 `VACUUM INTO` 可执行」的根因级证明）；
//! 本文件只钉壳层接线一步可观察的结果（文件被归一化 + 相位就绪）。

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

use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::encryption::EncryptionGate;
use ledger_infra::db::{self, DbState};
use ledger_infra::test_utils::{
    FOREIGN_RESERVED_BYTES, read_reserved_byte, write_foreign_form_plaintext_db,
};
use tauri::Manager;
use tauri_app_lib::commands::boot::{BootCell, get_boot_status, restart_app};

/// `$HOME` 现场隔离（本二进制进程内一次；tests/commands/isolation 同型）：
/// mock runtime 的 `app_data_dir()` 由 `$HOME` 派生，重定向后默认数据目录落在
/// 进程专属临时区，不触真实用户目录。
fn isolate_home() {
    static ISOLATE: std::sync::Once = std::sync::Once::new();
    ISOLATE.call_once(|| {
        let root =
            std::env::temp_dir().join(format!("ledger-boot-normalize-it-{}", db::new_uuid()));
        std::fs::create_dir_all(&root).unwrap();
        // SAFETY：set_var 自 Rust 2024 起 unsafe；本函数是本测试二进制唯一的
        // `$HOME` 写入点，Once 保证至多执行一次、写入后不再变化。
        unsafe { std::env::set_var("HOME", &root) };
    });
}

/// 引导登记态与两扇门（生产 setup 同型；mock 应用无 setup），库目录 = 默认
/// 数据目录（`restart_app` 原位重引导按同一路径解析）。
fn app_dir_device_app() -> (
    tauri::AppHandle<tauri::test::MockRuntime>,
    std::path::PathBuf,
) {
    // 写路径副作用与交易域接缝接线（与生产启动/测试工厂同形，幂等）。
    ledger_accounts::balance::install_balance_refresh_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let app = tauri::test::mock_app();
    let dir = app
        .path()
        .app_data_dir()
        .expect("mock app 应可解析 app_data_dir");
    std::fs::create_dir_all(&dir).unwrap();
    app.manage(BootCell::new(db::data_location::boot(&dir)));
    app.manage(EncryptionGate::new(false));
    app.manage(BootFailureGate::new());
    (app.handle().clone(), dir)
}

/// 验收判据（issue #1453）：以畸形夹具启动 → 相位 `ready`、库文件被归一化
/// （偏移 20 = 0）、夹具数据经归一化后的真实连接仍可读。
#[tokio::test(flavor = "multi_thread")]
async fn boot_normalizes_foreign_form_plaintext_ledger() {
    isolate_home();
    let (app, dir) = app_dir_device_app();
    let db_path = dir.join(db::data_location::DB_FILE_NAME);
    write_foreign_form_plaintext_db(&db_path, 3);
    assert_eq!(
        read_reserved_byte(&db_path),
        FOREIGN_RESERVED_BYTES,
        "前置：库文件应是外来形态（保留字节 12）"
    );

    restart_app(app.clone()).await.expect("原位重引导应成功");

    let status = get_boot_status(app.clone()).expect("启动状态应可读");
    assert_eq!(status.phase, "ready", "归一化后应落到主界面相位");
    assert_eq!(status.error_code, None);
    assert_eq!(
        read_reserved_byte(&db_path),
        0,
        "启动归一化应把库文件的每页保留字节归零（删除该接线即在此变红）"
    );
    // 归一化是整库重写：夹具数据必须完整地留在归一化后的库里，且应用连接读到
    // 的正是归一化后的库（一步可观察证明）。
    let state = app.state::<DbState>();
    {
        let conn = state.conn.lock().unwrap();
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM fixture_probe", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 3, "归一化后的库存应完整保留夹具数据");
    }
    // 用户可观察结果之二：**备份可用**。`VACUUM INTO` 正是自动备份与多端同步
    // 检查点产出的同一条语句，也是现场「全灭」的那条路径——归一化没生效
    //（接线被删）时它照旧失败，本用例随之变红。
    let snapshot = dir.join("snapshot.db");
    {
        let conn = db::open_connection(&db_path).unwrap();
        conn.execute_batch(&format!("VACUUM INTO '{}'", snapshot.display()))
            .expect("归一化后的库应能产出快照（备份可用）");
    }
    assert_eq!(
        read_reserved_byte(&snapshot),
        0,
        "备份产物应是干净形态，不把污染传给下一份库"
    );
}
