//! 同步低频轮询接线测试（issue #959 / ADR-0098 决策 1「桌面运行期低频轮询」）。
//!
//! **独立测试二进制**（不经 tests/commands 目标）：调度线程的单次拉起守卫与
//! 写信号通道是进程级单例，与写后触发接线测试（tests/commands/sync_trigger.rs，
//! 刻意注入超长轮询隔离信号路径）在同一进程内互斥——轮询行为断言须在独立
//! 进程现场注入「短轮询」。
//!
//! 断言对准用户可观察结果：通道已配置、零本地写入，注入短轮询周期后调度线程
//! 的 `recv_timeout` 到期分支自跑轮次，「上次成功同步时刻」落库（设置页同步
//! 卡片回显值，CONTEXT-testing〈断言强度〉）。

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

use std::time::{Duration, Instant};

use tauri::Manager;

use tauri_app_lib::commands::boot::BootCell;
use tauri_app_lib::db::boot::BootFailureGate;
use tauri_app_lib::db::data_location;
use tauri_app_lib::db::encryption::EncryptionGate;
use tauri_app_lib::db::{self, DbState};
use tauri_app_lib::settings::{self, SettingKey};
use tauri_app_lib::sync_engine::{SyncChannelConfig, TriggerTimings, start_sync_scheduler_with};
use tauri_app_lib::test_support::spawn_webdav_stub;

/// 低频轮询到期自跑轮次：无任何本地写入（无写信号），`recv_timeout` 超时分支
/// 兜底轮询 → 成功轮次把「上次同步时刻」落库。删掉调度线程的轮询分支（到期
/// 不再跑轮）本测试红。
#[tokio::test]
async fn poll_interval_elapses_into_a_round() {
    let stub = spawn_webdav_stub(Some(("alice", "app-pass")));

    // 设备现场（与 tests/commands/sync_channel.rs 同型）：mock 应用 + 独立
    // 临时目录文件库 + 引导登记态 + 两扇门（调度线程做空转判定）。
    let dir = std::env::temp_dir().join(format!(
        "ledger-sync-poll-it-{}",
        tauri_app_lib::db::new_uuid()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let app = tauri::test::mock_app();
    app.manage(BootCell::new(data_location::boot(&dir)));
    app.manage(db::open_db_in(&dir).unwrap());
    app.manage(EncryptionGate::new(false));
    app.manage(BootFailureGate::new());
    let app = app.handle().clone();

    // 通道配置经设置域单点落库（自动轮次只读配置；不经壳层命令——本票要钉的
    // 是轮询时机，配置写入面已由 tests/commands/sync_channel.rs 覆盖）。
    {
        let conn = app.state::<DbState>().conn.clone();
        let guard = conn.lock().unwrap();
        settings::set(
            &guard,
            SettingKey::SyncChannelConfig,
            &SyncChannelConfig {
                base_url: stub.base_url.clone(),
                username: "alice".into(),
                password: "app-pass".into(),
                space_id: "family".into(),
            },
        )
        .unwrap();
    }

    // 注入短轮询（生产 10 分钟，可逆工程决策 ADR-0091）；零写入现场——到期
    // 分支是本轮次唯一来路。
    start_sync_scheduler_with(
        &app,
        TriggerTimings {
            poll_interval: Duration::from_millis(200),
            write_debounce: Duration::from_millis(50),
        },
    );

    // 轮询等待用户可观察结果：上次成功同步时刻落库。
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
