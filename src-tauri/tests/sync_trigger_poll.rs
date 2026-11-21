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

use std::time::{Duration, Instant};

use tauri::Manager;

use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::data_location;
use ledger_infra::db::encryption::EncryptionGate;
use ledger_infra::db::{self, DbState};
use ledger_infra::settings::{self, SettingKey};
use ledger_sync_engine::{SyncChannelConfig, TriggerTimings, start_sync_scheduler_with};
use tauri_app_lib::commands::boot::BootCell;
use tauri_app_lib::test_support::{S3Addressing, S3StubConfig, ScratchDir, spawn_s3_stub};

#[tokio::test]
async fn poll_interval_elapses_into_a_round() {
    let stub = spawn_s3_stub(S3StubConfig::new(S3Addressing::PathStyle));

    let dir = ScratchDir::new("sync-poll-it");
    let app = tauri::test::mock_app();
    app.manage(BootCell::new(data_location::boot(&dir)));
    app.manage(db::open_db_in(&dir).unwrap());
    app.manage(EncryptionGate::new(false));
    app.manage(BootFailureGate::new());
    let app = app.handle().clone();

    {
        let conn = app.state::<DbState>().conn.clone();
        let guard = conn.lock().unwrap();
        settings::set(
            &guard,
            SettingKey::SyncChannelConfig,
            &SyncChannelConfig {
                endpoint: stub.endpoint.clone(),
                region: stub.region.clone(),
                bucket: stub.bucket.clone(),
                access_key: stub.access_key.clone(),
                secret_key: "test-secret-key".into(),
                prefix: String::new(),
                path_style: true,
                space_id: "family".into(),
            },
        )
        .unwrap();
    }

    start_sync_scheduler_with(
        &app,
        TriggerTimings {
            poll_interval: Duration::from_millis(200),
            write_debounce: Duration::from_millis(50),
        },
    );

    let conn = app.state::<DbState>().conn.clone();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let stamped = {
            let guard = conn.lock().unwrap();
            settings::get::<Option<String>>(&guard, SettingKey::SyncLastSyncAt, None)
                .unwrap()
                .is_some()
        };
        if stamped {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "低频轮询未在限时内跑出成功轮次（上次同步时刻未落库）"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
