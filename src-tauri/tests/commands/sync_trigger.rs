use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use tauri::Manager;

use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::encryption::EncryptionGate;
use ledger_infra::db::{self, DbState};
use ledger_infra::settings::{self, SettingKey};
use ledger_sync_engine::{
    ChannelManifest, TriggerTimings, build_channel, configured_channel, start_sync_scheduler_with,
    sync_on_start,
};
use tauri_app_lib::commands::{accounts, transactions};

use crate::sync_channel::{configure_channel, expense_input, fresh_app, spawn_sync_stub};
use tauri_app_lib::test_support::isolate_home;

fn trigger_device_app(
    tag: &str,
) -> (
    tauri::AppHandle<tauri::test::MockRuntime>,
    tauri_app_lib::test_support::ScratchDir,
) {
    let (app, dir) = fresh_app(tag);
    app.manage(db::open_db_in(&dir).unwrap());
    app.manage(EncryptionGate::new(false));
    app.manage(BootFailureGate::new());
    (app.handle().clone(), dir)
}

async fn seed_own_ops(app: &tauri::AppHandle<tauri::test::MockRuntime>) {
    let acc_id = accounts::create_account(
        app.state(),
        app.clone(),
        ledger_accounts::AccountInput {
            name: "现金".into(),
            kind: ledger_accounts::AccountType::Cash,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(0),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .await
    .expect("建户应成功");
    transactions::create_transaction(
        app.state(),
        app.clone(),
        expense_input(&acc_id, 10_000, "触发接线的账"),
    )
    .await
    .expect("记账应成功");
}

fn wait_for_auto_round(conn: Arc<Mutex<Connection>>, space_id: &'static str) {
    let worker = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let (mut stamped, mut segment) = (false, false);
        while Instant::now() < deadline {
            let channel = {
                let guard = conn.lock().unwrap();
                let config = configured_channel(&guard)
                    .expect("通道配置读取应成功")
                    .expect("现场应已配置通道");
                build_channel(&config).expect("通道构库应成功")
            };
            let device_id: String = {
                let guard = conn.lock().unwrap();
                guard
                    .query_row("SELECT id FROM sync_device LIMIT 1", [], |r| r.get(0))
                    .expect("本机设备标识应已生成")
            };
            stamped = {
                let guard = conn.lock().unwrap();
                settings::get::<Option<String>>(&guard, SettingKey::SyncLastSyncAt, None)
                    .expect("读上次同步时刻应成功")
                    .is_some()
            };
            let manifest: Option<ChannelManifest> = channel
                .transport()
                .read_file(&channel.layout().manifest_path())
                .expect("读通道 manifest 应成功")
                .and_then(|bytes| serde_json::from_slice(&bytes).ok());
            segment = manifest.is_some_and(|manifest| {
                manifest
                    .streams
                    .iter()
                    .any(|stream| stream.device_id == device_id && !stream.segments.is_empty())
            });
            if stamped && segment {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!(
            "自动轮次未在限时内产生可观察结果：上次同步时刻落库 = {stamped}，通道上本机段 = {segment}（space {space_id}）"
        );
    });
    match worker.join() {
        Ok(()) => {}
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

#[tokio::test]
async fn write_entry_enqueues_upload_via_scheduler() {
    isolate_home();
    let stub = spawn_sync_stub();
    let (app, _dir) = trigger_device_app("write-after");
    configure_channel(&app, &stub);

    start_sync_scheduler_with(
        &app,
        TriggerTimings {
            poll_interval: Duration::from_secs(600),
            write_debounce: Duration::from_millis(50),
        },
    );

    seed_own_ops(&app).await;

    let conn = app.state::<DbState>().conn.clone();
    wait_for_auto_round(conn, "family");
}

#[tokio::test]
async fn sync_on_start_publishes_local_ops_to_channel() {
    isolate_home();
    let stub = spawn_sync_stub();
    let (app, _dir) = trigger_device_app("on-start");
    configure_channel(&app, &stub);

    seed_own_ops(&app).await;

    sync_on_start(&app);

    let conn = app.state::<DbState>().conn.clone();
    wait_for_auto_round(conn, "family");
}
