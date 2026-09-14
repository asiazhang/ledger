//! 读路径独立只读连接集成测试（issue #1280 / ADR-0117 路线 B 实施票）：钉住
//! 壳层换连接线的**用户可观察结果**——
//!
//! 1. **长写任务在途时读及时返回**（验收判据①）：写连接锁被长写事务占用时，
//!    读命令限时返回当前已提交数据。负向判据（ADR-0087 断言强度）：读路径若
//!    退回写连接（共用互斥锁），本测试在限时内拿不到结果即红。
//! 2. **加密解锁成对换入**（验收判据②）：解锁成功后读连接与写连接以同一口令
//!    成对换入——删除读连接换入调用，读命令拿不到真实库数据即红。
//! 3. **恢复换出 + 重引导成对换入**（验收判据③）：恢复替换库文件后读连接立即
//!    换出（读报错而非旧库数据），原位重引导按新库文件成对换入——删除换出
//!    调用，恢复后到重启前读到旧库数据即红。
//! 4. **加密转换换出 + 解锁换入**（验收判据③同族）：整库转换替换文件后读连接
//!    立即换出，重启落解锁屏，解锁后成对换入。
//!
//! **独立测试二进制**（不经 tests/commands 目标，sync_trigger_poll 同款理由）：
//! 场景②③④经换连编排拉起后台服务（`start_background_services`，issue #961
//! 唯一编排点），调度线程的单次拉起守卫是进程级单例——与 tests/commands 内
//! 刻意注入时机的 sync_trigger 接线测试在同一进程内互斥（先行的拉起会让其
//! 注入失效）。
//!
//! 现场形态与 tests/commands 同款：mock 应用 + 独立临时目录文件库 + 产品建连缝
//! 成对打开（`db::open_db_in`）。场景③④经 `restart_app` 原位重引导，引导的
//! 默认数据目录是 mock runtime 的 `app_data_dir()`（进程内固定路径，books
//! 先例），故两场景收进一个顺序旅程测试；断言对准命令面可观察结果，不对准
//! 线程或函数调用形状（CONTEXT-testing〈断言强度〉）。

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

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use tauri::Manager;
type AppHandle = tauri::AppHandle<tauri::test::MockRuntime>;

use tauri_app_lib::commands::accounts;
use tauri_app_lib::commands::backup::{create_backup, restore_backup};
use tauri_app_lib::commands::boot::{get_boot_status, restart_app};
use tauri_app_lib::commands::dashboard::dashboard_overview;
use tauri_app_lib::commands::encryption::{enable_encryption, unlock_encryption};
use tauri_app_lib::db::boot::BootFailureGate;
use tauri_app_lib::db::encryption::EncryptionGate;
use tauri_app_lib::db::{self, DbState};

/// `$HOME` 现场隔离（本二进制进程内一次；tests/commands/isolation 同型）：
/// mock runtime 的 `app_data_dir()` 由 `$HOME` 派生，重定向后默认数据目录
/// 落在进程专属临时区，不触真实用户目录。
fn isolate_home() {
    static ISOLATE: std::sync::Once = std::sync::Once::new();
    ISOLATE.call_once(|| {
        let root = std::env::temp_dir().join(format!("ledger-readonly-it-{}", db::new_uuid()));
        std::fs::create_dir_all(&root).unwrap();
        // SAFETY：set_var 自 Rust 2024 起 unsafe；本函数是本测试二进制唯一的
        // `$HOME` 写入点，Once 保证至多执行一次、写入后不再变化。
        unsafe { std::env::set_var("HOME", &root) };
    });
}

/// 明文库现场：mock 应用 + 引导登记态 + 临时目录文件库成对打开 + 两扇门
/// （与生产 setup / tests/commands 同型）。
fn readonly_device_app(tag: &str, locked: bool) -> (AppHandle, PathBuf) {
    let dir = std::env::temp_dir().join(format!("ledger-readonly-it-{tag}-{}", db::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let app = tauri::test::mock_app();
    app.manage(tauri_app_lib::commands::boot::BootCell::new(
        db::data_location::boot(&dir),
    ));
    // 写路径副作用与交易域接缝接线（与测试工厂/生产启动同形，幂等）。
    tauri_app_lib::transaction_wiring::install_all();
    app.manage(db::open_db_in(&dir).unwrap());
    app.manage(EncryptionGate::new(locked));
    app.manage(BootFailureGate::new());
    (app.handle().clone(), dir)
}

/// mock 应用 + 真实引导态，库目录 = `app_data_dir()`（引导序列解析的默认数据
/// 目录，books.rs 同款——`restart_app` 原位重引导按同一路径成对换连）。
fn app_dir_device_app() -> (tauri::App<tauri::test::MockRuntime>, PathBuf) {
    let app = tauri::test::mock_app();
    let dir = app
        .path()
        .app_data_dir()
        .expect("mock app 应可解析 app_data_dir");
    std::fs::create_dir_all(&dir).unwrap();
    let boot = tauri_app_lib::db::data_location::boot(&dir);
    app.manage(tauri_app_lib::commands::boot::BootCell::new(boot));
    app.manage(EncryptionGate::new(false));
    app.manage(BootFailureGate::new());
    (app, dir)
}

/// 经壳层公开命令建户（行为前置走公开入口）。
async fn seed_account(app: &AppHandle, name: &str, cents: i64) -> String {
    accounts::create_account(
        app.state(),
        app.clone(),
        tauri_app_lib::accounts::AccountInput {
            name: name.into(),
            kind: tauri_app_lib::accounts::AccountType::Cash,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(cents),
        },
    )
    .await
    .expect("建户应成功")
}

/// 限时内读取账户列表并返回（超时即失败：读路径被写者闸门挡住的用户可观察形态）。
async fn list_accounts_within(
    app: &AppHandle,
    limit: Duration,
) -> Vec<tauri_app_lib::accounts::Account> {
    tokio::time::timeout(limit, accounts::list_accounts(app.state()))
        .await
        .expect("读命令应在限时内返回（读不得被写者闸门无界阻挡）")
        .expect("读命令应成功（读连接应指向真实库）")
}

/// 验收判据①：长写事务在途（占写连接锁）时，读命令限时返回当前已提交数据。
///
/// 负向判据：读退回写连接（同锁）则本测试超时变红；删除读连接换入接线则
/// 读命令报错变红。
#[tokio::test(flavor = "multi_thread")]
async fn read_returns_while_long_write_holds_connection_lock() {
    isolate_home();
    let (app, _dir) = readonly_device_app("ro-blocking", false);
    seed_account(&app, "已提交账户", 12_345).await;

    // 长写事务在途：后台线程持写连接锁并打开写事务（RESERVED），持有期间
    // 「写者闸门」不可用——与真实长写任务在途同形。
    let write_conn = app.state::<DbState>().conn.clone();
    let (locked_tx, locked_rx) = mpsc::channel::<()>();
    let writer = std::thread::spawn(move || {
        let conn = write_conn.lock().unwrap();
        conn.execute_batch("BEGIN IMMEDIATE").unwrap();
        locked_tx.send(()).unwrap();
        std::thread::sleep(Duration::from_secs(3));
        conn.execute_batch("ROLLBACK").unwrap();
    });
    locked_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("写事务应在限时内在途");

    let started = Instant::now();
    let rows = list_accounts_within(&app, Duration::from_secs(2)).await;
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "读应在写事务在途时即时返回，实际耗时 {:?}",
        started.elapsed()
    );
    assert_eq!(rows.len(), 1, "应读到当前已提交数据（不含在途写入）");
    assert_eq!(rows[0].name, "已提交账户");

    writer.join().expect("写事务线程应正常收尾");
}

/// 只读甄别收口的守护（issue #1280 / ADR-0117 代价 3）：读形态但闭包内含惰性写
/// 的命令（仪表盘缓存自愈）必须走写连接——写后指纹变化（缓存失效），命令在
/// 成对连接现场仍应成功（走错读连接则在缓存自愈 UPSERT 上报只读错误）。
#[tokio::test(flavor = "multi_thread")]
async fn lazy_write_read_command_stays_on_write_connection() {
    isolate_home();
    let (app, _dir) = readonly_device_app("ro-lazy-write", false);
    // 首次调用：缓存冷启动，读探针实时重算并回填缓存（写）。
    let overview = dashboard_overview(app.state()).await.expect("仪表盘应可读");
    let _ = overview;
    // 再次写入：指纹变化使缓存失效，下次读触发缓存自愈写。
    seed_account(&app, "缓存失效账户", 999).await;
    dashboard_overview(app.state())
        .await
        .expect("缓存失效后的仪表盘读应成功（甄别路由到写连接，不得走只读连接）");
}

/// 验收判据②：密文库解锁后读连接与写连接以同一口令成对换入——解锁成功后
/// 读命令读到真实库数据。
///
/// 负向判据：删除解锁编排的读连接换入调用，读连接残留占位形态，读命令拿
/// 不到新写入的真实库数据即红。
#[tokio::test(flavor = "multi_thread")]
async fn unlock_swaps_in_read_connection_pair() {
    isolate_home();
    let (app, dir) = readonly_device_app("ro-unlock", true);
    // 密文库现场：明文库建好即弃连接 → 原位转密文 → 以锁定态 + 占位连接对进入
    //（与 boot_sequence AwaitUnlock 相位同形：门立起、占位对维持形状）。
    let db_path = dir.join(db::data_location::DB_FILE_NAME);
    drop(db::open_db_in(&dir).unwrap());
    tauri_app_lib::db::encryption::enable_encryption_for_file(&db_path, "主口令").unwrap();
    std::fs::remove_file(db_path.with_extension("db.bak")).unwrap();
    // 占位连接对：空目录文件库成对打开（维持 DbState 形状，非业务库）。
    let placeholder_dir = dir.join("placeholder-shape");
    std::fs::create_dir_all(&placeholder_dir).unwrap();
    app.manage(db::open_db_in(&placeholder_dir).unwrap());

    // 解锁成功：成对换入 + 门翻转（命令内含口令验证）。
    unlock_encryption(app.clone(), "主口令".into())
        .await
        .expect("解锁应成功");

    // 解锁后经公开命令写入并读回：读连接必须指向真实密文库（残留占位即红）。
    seed_account(&app, "解锁后账户", 5_000).await;
    let rows = list_accounts_within(&app, Duration::from_secs(2)).await;
    let names: Vec<&str> = rows.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, vec!["解锁后账户"], "解锁后读连接应读到真实库数据");
}

/// 验收判据③（恢复 + 加密转换，顺序旅程）：文件替换成功后读连接立即换出
///（读报错，不读旧库），原位重引导按新库文件成对换入。
///
/// 负向判据：删除恢复/转换的读连接换出调用，重启前读命令会拿到旧库数据而非
/// 报错，断言 `is_err()` 即红；删除重引导的成对换入，重启后读命令报错或内容
/// 不符即红。
#[tokio::test(flavor = "multi_thread")]
async fn file_replacement_swaps_read_conn_out_then_reboot_swaps_pair_back_in() {
    isolate_home();
    let (app, dir) = app_dir_device_app();
    let app = app.handle().clone();
    let db_path = dir.join(tauri_app_lib::db::data_location::DB_FILE_NAME);
    app.manage(db::open_db_in(&dir).unwrap());

    // ---------- 场景一：恢复 ----------
    // 备份时点：只有「备份时账户」。
    let target = dir.join("manual-backup.zip");
    seed_account(&app, "备份时账户", 1_000).await;
    create_backup(app.clone(), target.to_string_lossy().into_owned())
        .await
        .expect("备份应成功");
    // 备份后继续写入：恢复应回到只有「备份时账户」的状态。
    seed_account(&app, "备份后账户", 2_000).await;

    // 恢复成功：文件已替换，读连接立即换出——读命令报错（不是旧库数据）。
    restore_backup(app.clone(), target.to_string_lossy().into_owned(), None)
        .await
        .expect("恢复应成功");
    let stale_read =
        tokio::time::timeout(Duration::from_secs(2), accounts::list_accounts(app.state()))
            .await
            .expect("换出后读命令应即时返回（不被写者闸门阻挡）");
    assert!(
        stale_read.is_err(),
        "恢复后、重引导前读连接应已换出：读必须报错而非拿到旧库数据，实际 {stale_read:?}"
    );

    // 原位重引导（前端既有触发的同一入口）：按恢复后的新库文件成对换入。
    restart_app(app.clone()).await.expect("重引导应成功");
    let rows = list_accounts_within(&app, Duration::from_secs(2)).await;
    let names: Vec<&str> = rows.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["备份时账户"],
        "重引导后读连接应读到恢复结果（新库文件成对换入）"
    );

    // ---------- 场景二：开启加密转换 ----------
    enable_encryption(app.clone(), "新口令".into())
        .await
        .expect("加密转换应成功");
    assert_eq!(
        tauri_app_lib::db::encryption::probe_file_kind(&db_path).unwrap(),
        tauri_app_lib::db::encryption::DbFileKind::Encrypted,
        "转换产物应为密文库"
    );
    let stale_read =
        tokio::time::timeout(Duration::from_secs(2), accounts::list_accounts(app.state()))
            .await
            .expect("换出后读命令应即时返回（不被写者闸门阻挡）");
    assert!(
        stale_read.is_err(),
        "转换后、重引导前读连接应已换出：读必须报错而非拿到旧库数据，实际 {stale_read:?}"
    );

    // 重启落解锁屏（密文库 → AwaitUnlock），解锁后成对换入，数据原样可读。
    restart_app(app.clone()).await.expect("重引导应成功");
    let status = get_boot_status(app.clone()).expect("启动状态应可读");
    assert_eq!(status.phase, "locked", "密文库重引导应落解锁屏");
    let locked_read =
        tokio::time::timeout(Duration::from_secs(2), accounts::list_accounts(app.state()))
            .await
            .expect("锁定期间读命令应即时返回");
    assert!(
        locked_read.is_err(),
        "锁定期间读连接是占位形态：读必须报错而非任何库数据，实际 {locked_read:?}"
    );
    unlock_encryption(app.clone(), "新口令".into())
        .await
        .expect("解锁应成功");
    let rows = list_accounts_within(&app, Duration::from_secs(2)).await;
    let names: Vec<&str> = rows.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["备份时账户"],
        "解锁后读连接应读到转换前的原样数据"
    );
}
