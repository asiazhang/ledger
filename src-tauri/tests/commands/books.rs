//! 账本注册表命令面集成测试（issue #833 / ADR-0089）。
//!
//! 直调命令函数（`#[tokio::test]`），覆盖壳行为：参数解包、错误码、切换后
//! 的引导相位（引导序列内核 `db::boot::plan_boot`）。登记规则的纯函数语义
//! 归内核单测（`db::book_registry` / `db::data_location`），此处只钉
//! 「命令壳 → 内核 → 落盘 → 引导」整链。
//!
//! 现场隔离：mock runtime 的 `app_data_dir()` 实现为 `dirs::data_dir()` 拼接
//! 空 identifier（`$HOME/Library/Application Support`），指向真实用户目录。
//! 进程启动时把 `$HOME` 重定向到本测试目标专属临时目录，默认数据目录随之
//! 落在隔离区。tokio 测试线程池并行执行会互踩同一现场，故全部场景收进
//! 一个顺序旅程测试（与 BDD 场景流同型）。

use std::path::PathBuf;
use std::sync::Once;

use tauri::Manager;
use tauri_app_lib::commands::book;
use tauri_app_lib::commands::boot::BootCell;
use tauri_app_lib::db::boot::plan_boot;
use tauri_app_lib::db::data_location;
use tauri_app_lib::db::data_location::DB_FILE_NAME;
use tauri_app_lib::db::encryption::enable_encryption_for_file;
use tauri_app_lib::error::AppError;

static ISOLATE: Once = Once::new();

/// HOME 重定向（进程内一次）：mock runtime 经 `dirs::data_dir()` 解析
/// `app_data_dir`，macOS/Linux 下该值由 `$HOME` 派生。重定向后命令壳解析出
/// 的默认数据目录落在进程专属临时目录，绝不触真实用户目录。
///
/// SAFETY：`set_var` 自 Rust 2024 起 unsafe。调用点在各测试函数首行、断言
/// 现场文件操作之前；本测试目标内无其他代码并发读取 `$HOME`（无多线程
/// 现场依赖）， Once 保证只执行一次，之后整个进程的目录解析都落在隔离区。
fn isolate_home() {
    ISOLATE.call_once(|| {
        let root = std::env::temp_dir().join(format!(
            "ledger-commands-it-{}",
            tauri_app_lib::db::new_uuid()
        ));
        std::fs::create_dir_all(&root).unwrap();
        // SAFETY：见函数文档。
        unsafe { std::env::set_var("HOME", &root) };
    });
}

/// mock 应用 + 真实引导态：引导按生产同型登记（真实
/// [`data_location::boot`] 解析 → [`BootCell`]）——命令预检与清单聚合
/// 消费的正是这一引导态。
fn mock_app() -> (tauri::AppHandle<tauri::test::MockRuntime>, PathBuf) {
    let app = tauri::test::mock_app();
    let dir = app
        .path()
        .app_data_dir()
        .expect("mock app 应可解析 app_data_dir");
    std::fs::create_dir_all(&dir).unwrap();
    let boot = data_location::boot(&dir);
    app.manage(BootCell::new(boot));
    (app.handle().clone(), dir)
}

/// 断言码化错误命中的稳定码。
fn assert_code(err: AppError, code: &str) {
    assert!(err.is_code(code), "期望 {code}，实际 {err:?}");
}

#[tokio::test]
async fn book_registry_command_surface_end_to_end() {
    isolate_home();

    // -----------------------------------------------------------------
    // 场景一：新建 → 清单 → 切换 → 明文库引导直进相位。
    // -----------------------------------------------------------------
    let (app, dir) = mock_app();
    let created = book::create_book(app.clone(), "家庭账本".into())
        .await
        .expect("新建应成功");
    assert_eq!(created.name, "家庭账本");
    assert!(created.dir.is_dir(), "新建应自动创建账本目录");

    let info = book::list_books(app.clone()).await.expect("清单应可用");
    assert!(info.mutable);
    assert!(info.fallback_reason.is_none());
    assert_eq!(info.books.len(), 2); // 默认账本 + 家庭账本
    assert_ne!(info.active_id.as_deref(), Some(created.id.as_str()));

    let switched = book::switch_book(app.clone(), created.id.clone())
        .await
        .expect("切换应成功");
    assert_eq!(switched.id, created.id);
    // 切换写活动指针落盘：随后的引导按注册表进入目标账本（明文直进相位）。
    let plan = plan_boot(&dir);
    assert_eq!(plan.boot.db_dir, created.dir);
    assert!(plan.boot.fallback_reason.is_none());
    assert!(
        plan.disposition.is_ok(),
        "明文库应直进：{:?}",
        plan.disposition
    );

    // -----------------------------------------------------------------
    // 场景二：切换到密文库 → 重引导落解锁屏相位。
    // -----------------------------------------------------------------
    let (app, dir) = mock_app();
    let encrypted = book::create_book(app.clone(), "密码本".into())
        .await
        .expect("新建应成功");
    // 现场造密文库：产品入口建真库（含迁移，文件落盘）后转密文（e2e 同款接缝）。
    let db_state = tauri_app_lib::db::open_db_in(&encrypted.dir).unwrap();
    drop(db_state);
    enable_encryption_for_file(&encrypted.dir.join(DB_FILE_NAME), "pw-1234").unwrap();

    book::switch_book(app.clone(), encrypted.id.clone())
        .await
        .expect("切换应成功");
    let plan = plan_boot(&dir);
    assert_eq!(plan.boot.db_dir, encrypted.dir);
    assert!(
        matches!(
            plan.disposition,
            Ok(tauri_app_lib::db::boot::BootDisposition::AwaitUnlock)
        ),
        "密文库应落解锁屏相位，实际 {:?}",
        plan.disposition
    );

    // -----------------------------------------------------------------
    // 场景三：改名 → 移除（仅摘登记、文件保留）。
    // -----------------------------------------------------------------
    let (app, _dir) = mock_app();
    let created = book::create_book(app.clone(), "临时名".into())
        .await
        .expect("新建应成功");
    let renamed = book::rename_book(app.clone(), created.id.clone(), "正式名".into())
        .await
        .expect("改名应成功");
    assert_eq!(renamed.name, "正式名");
    book::remove_book(app.clone(), created.id.clone())
        .await
        .expect("移除非活动账本应成功");
    let info = book::list_books(app).await.expect("清单应可用");
    assert!(info.books.iter().all(|b| b.id != created.id));
    assert!(created.dir.is_dir(), "移除仅摘登记，目录必须保留");

    // -----------------------------------------------------------------
    // 场景四：错误码矩阵（参数解包层稳定码）。
    // -----------------------------------------------------------------
    let (app, _dir) = mock_app();
    book::create_book(app.clone(), "一本".into())
        .await
        .expect("新建应成功");
    assert_code(
        book::create_book(app.clone(), "   ".into())
            .await
            .unwrap_err(),
        "book.name-required",
    );
    assert_code(
        book::switch_book(app.clone(), "no-such-id".into())
            .await
            .unwrap_err(),
        "book.not-found",
    );
    assert_code(
        book::rename_book(app.clone(), "no-such-id".into(), "新名".into())
            .await
            .unwrap_err(),
        "book.not-found",
    );
    // 活动账本不可移除（新建后活动账本仍是默认账本）→ 稳定错误码。
    let active_id = book::list_books(app.clone())
        .await
        .expect("清单应可用")
        .active_id
        .expect("正常注册表应有活动账本");
    assert_code(
        book::remove_book(app.clone(), active_id.clone())
            .await
            .unwrap_err(),
        "book.remove-active",
    );
    // 切换到不可用目录：目录被普通文件占位后无法创建 → 稳定错误码，指针不变。
    let other = book::create_book(app.clone(), "二本".into())
        .await
        .expect("新建应成功");
    std::fs::remove_dir_all(&other.dir).unwrap();
    std::fs::write(&other.dir, b"not a dir").unwrap();
    assert_code(
        book::switch_book(app.clone(), other.id.clone())
            .await
            .unwrap_err(),
        "book.dir-unavailable",
    );

    // -----------------------------------------------------------------
    // 场景五：注册表损坏 → 清单降级（空 + 不可变 + 回退原因），变更拒绝。
    // -----------------------------------------------------------------
    let dir = mock_app().1;
    std::fs::write(dir.join(data_location::POINTER_FILE_NAME), "{broken").unwrap();
    let (app, _) = mock_app_with_boot(data_location::boot(&dir));
    let info = book::list_books(app.clone()).await.expect("清单应可用");
    assert!(info.books.is_empty(), "损坏时清单不可信");
    assert_eq!(info.active_id, None);
    assert!(!info.mutable);
    assert!(info.fallback_reason.is_some());
    assert_code(
        book::create_book(app, "新账本".into()).await.unwrap_err(),
        "book.registry-corrupt",
    );

    // -----------------------------------------------------------------
    // 场景六：推迟搬迁窗口 → 变更一律拒绝（registry-busy）。
    // -----------------------------------------------------------------
    let dir = mock_app().1;
    let mut deferred = data_location::boot(&dir);
    deferred.deferred_relocation = Some(PathBuf::from("/somewhere/target"));
    let (app, _dir) = mock_app_with_boot(deferred);
    assert_code(
        book::create_book(app.clone(), "新账本".into())
            .await
            .unwrap_err(),
        "book.registry-busy",
    );
    assert_code(
        book::switch_book(app, "any-id".into()).await.unwrap_err(),
        "book.registry-busy",
    );
}

/// 在指定引导态上建 mock 应用（场景五：损坏文件后引导；场景六：注入
/// 推迟搬迁窗口的引导态）。
fn mock_app_with_boot(
    boot: data_location::Boot,
) -> (tauri::AppHandle<tauri::test::MockRuntime>, PathBuf) {
    let app = tauri::test::mock_app();
    let dir = app
        .path()
        .app_data_dir()
        .expect("mock app 应可解析 app_data_dir");
    std::fs::create_dir_all(&dir).unwrap();
    app.manage(BootCell::new(boot));
    (app.handle().clone(), dir)
}
