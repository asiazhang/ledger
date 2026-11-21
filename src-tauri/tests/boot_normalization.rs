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
use tauri_app_lib::test_support::isolate_home;

fn app_dir_device_app() -> (
    tauri::AppHandle<tauri::test::MockRuntime>,
    std::path::PathBuf,
) {
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
    let state = app.state::<DbState>();
    {
        let conn = state.conn.lock().unwrap();
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM fixture_probe", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 3, "归一化后的库存应完整保留夹具数据");
    }
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
