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
use ledger_infra::db::{self};
use ledger_sync_engine::SessionEnvelope;
use tauri_app_lib::commands::boot::{BootCell, reset_after_startup_failure};
use tauri_app_lib::commands::encryption::{reset_after_forgotten_passphrase, unlock_encryption};
use tauri_app_lib::test_support::isolate_home;

type AppHandle = tauri::AppHandle<tauri::test::MockRuntime>;

fn locked_encrypted_device_app(
    tag: &str,
    passphrase: &str,
) -> (AppHandle, tauri_app_lib::test_support::ScratchDir) {
    let dir = tauri_app_lib::test_support::ScratchDir::new(&format!("sessionenv-it-{tag}"));
    let app = tauri::test::mock_app();
    app.manage(BootCell::new(db::data_location::boot(&dir)));
    app.manage(EncryptionGate::new(true));
    app.manage(BootFailureGate::new());
    let db_path = dir.join(db::data_location::DB_FILE_NAME);
    drop(db::open_db_in(&dir).unwrap());
    ledger_infra::db::encryption::enable_encryption_for_file(&db_path, passphrase).unwrap();
    std::fs::remove_file(db_path.with_extension("db.bak")).unwrap();
    let placeholder_dir = dir.join("placeholder-shape");
    std::fs::create_dir_all(&placeholder_dir).unwrap();
    app.manage(db::open_db_in(&placeholder_dir).unwrap());
    (app.handle().clone(), dir)
}

fn failed_device_app() -> (AppHandle, tauri_app_lib::test_support::ScratchDir) {
    let dir = tauri_app_lib::test_support::ScratchDir::new("sessionenv-it-failed");
    let app = tauri::test::mock_app();
    app.manage(BootCell::new(db::data_location::boot(&dir)));
    app.manage(EncryptionGate::new(false));
    let gate = BootFailureGate::new();
    gate.set_failed(None);
    app.manage(gate);
    let placeholder_dir = dir.join("placeholder-shape");
    std::fs::create_dir_all(&placeholder_dir).unwrap();
    app.manage(db::open_db_in(&placeholder_dir).unwrap());
    (app.handle().clone(), dir)
}

#[tokio::test(flavor = "multi_thread")]
async fn resume_surfaces_declare_session_envelope_in_signature() {
    isolate_home();

    {
        let (app, _dir) = locked_encrypted_device_app("unlock", "解锁主口令");
        unlock_encryption(app.clone(), "解锁主口令".into())
            .await
            .expect("解锁应成功");
        assert_eq!(
            SessionEnvelope::current(),
            SessionEnvelope::Encrypted("解锁主口令".into()),
            "解锁成功后本会话信封应记入密文形态（resume 签名声明，自动轮次据此封包）"
        );
    }

    {
        let (app, _dir) = locked_encrypted_device_app("forgot-reset", "待重置主口令");
        reset_after_forgotten_passphrase(app.clone())
            .await
            .expect("忘记口令重置应成功");
        assert_eq!(
            SessionEnvelope::current(),
            SessionEnvelope::Plaintext,
            "忘记口令重置后本会话信封应记入明文形态（新库是明文空库、形态已知，\
             不再清空等待重新记入）"
        );
    }

    {
        let (app, _dir) = failed_device_app();
        SessionEnvelope::remember(SessionEnvelope::Encrypted("旧会话口令".into()));
        reset_after_startup_failure(app.clone())
            .await
            .expect("启动失败重置应成功");
        assert_eq!(
            SessionEnvelope::current(),
            SessionEnvelope::Plaintext,
            "启动失败重置后本会话信封应记入明文形态（新库是明文空库，声明的不变量）"
        );
    }
}
