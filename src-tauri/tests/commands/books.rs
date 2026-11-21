use std::path::PathBuf;

use ledger_infra::db::boot::plan_boot;
use ledger_infra::db::data_location;
use ledger_infra::db::data_location::DB_FILE_NAME;
use ledger_infra::db::encryption::enable_encryption_for_file;
use ledger_infra::error::AppError;
use tauri::Manager;
use tauri_app_lib::commands::book;
use tauri_app_lib::commands::boot::BootCell;

use tauri_app_lib::test_support::isolate_home;

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

fn assert_code(err: AppError, code: &str) {
    assert!(err.is_code(code), "期望 {code}，实际 {err:?}");
}

#[tokio::test]
async fn book_registry_command_surface_end_to_end() {
    isolate_home();

    let (app, dir) = mock_app();
    let created = book::create_book(app.clone(), "家庭账本".into())
        .await
        .expect("新建应成功");
    assert_eq!(created.name, "家庭账本");
    assert!(created.dir.is_dir(), "新建应自动创建账本目录");

    let info = book::list_books(app.clone()).await.expect("清单应可用");
    assert!(info.mutable);
    assert!(info.fallback_reason.is_none());
    assert_eq!(info.books.len(), 2);
    assert_ne!(info.active_id.as_deref(), Some(created.id.as_str()));

    let switched = book::switch_book(app.clone(), created.id.clone())
        .await
        .expect("切换应成功");
    assert_eq!(switched.id, created.id);
    let plan = plan_boot(&dir);
    assert_eq!(plan.boot.db_dir, created.dir);
    assert!(plan.boot.fallback_reason.is_none());
    assert!(
        plan.disposition.is_ok(),
        "明文库应直进：{:?}",
        plan.disposition
    );

    let (app, dir) = mock_app();
    let encrypted = book::create_book(app.clone(), "密码本".into())
        .await
        .expect("新建应成功");
    let db_state = ledger_infra::db::open_db_in(&encrypted.dir).unwrap();
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
            Ok(ledger_infra::db::boot::BootDisposition::AwaitUnlock)
        ),
        "密文库应落解锁屏相位，实际 {:?}",
        plan.disposition
    );

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
