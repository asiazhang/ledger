//! 写后触发接线（issue #2080 / issue #863 验收「写 op 后即时入队上传」的接线
//! 半边，旧壳 tests/commands/sync_trigger.rs::write_entry_enqueues_upload_via_scheduler
//! 的语义在 NSH 侧重生）：接线单点拉起调度线程（注入短去抖）→ 经域公开写入口
//! 记账 → `record_local` 写信号 → 去抖合流 → 轮次发布 → 通道上出现本机段 +
//! 上次同步时刻落库。删掉接线单点（或 `wiring.rs` 的写后钩子、或域侧写信号
//! 投递、或启动竖井的接线调用），本测试红（轮询周期超长，救不了场）。
//!
//! 独立测试二进制：调度线程单次拉起守卫是进程级先到先得，时机注入（短去抖）
//! 只在本二进制唯一场景即首个 wire 调用方时才被兑现——与旅程二进制
//! （tests/sync_trigger.rs，走生产默认时机）分进程互不污染（旧壳
//! sync_trigger.rs / sync_trigger_poll.rs 分进程同因）。
//!
//! 时机参数接缝：启动竖井显式时机版 [`nsh_app::app::run_startup_with_trigger_timings`]
//! （issue #959 显式参数接缝先例——参数是调度线程的正常输入，不是测试后门）。
//! 断言对准用户可观察结果（通道上出现本机段、上次同步时刻落库），不对准线程
//! 或函数调用形状（CONTEXT-testing〈断言强度〉）。

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

use std::sync::Arc;
use std::time::{Duration, Instant};

use ledger_infra::settings::{self, SettingKey};
use ledger_sync_engine::{TriggerTimings, build_channel, configured_channel};

#[path = "support/seed.rs"]
mod seed;

// 同步触发测试的 S3 桩 facade（模块名必须是 test_support：s3.rs 内
// `use crate::test_support::ScratchDir` 以 crate 根定位，见 support/s3_stub.rs
// 头注）。
#[path = "support/s3_stub.rs"]
mod test_support;

/// 经设置域单点直落通道配置（桩是明文 http 端点，保存门只收 https——配置
/// 面的保存/拒绝路径归 headless 同步卡旅程既有覆盖）。
fn configure_channel(db: &ledger_infra::db::DbState, config: &ledger_sync_engine::SyncChannelConfig) {
    let conn = db.conn.lock().unwrap();
    settings::set(&conn, SettingKey::SyncChannelConfig, config).unwrap();
}

/// 等待自动轮次的用户可观察结果并断言（限时轮询，OS 线程执行）：①「上次
/// 成功同步时刻」落库；②通道 manifest 上出现本机设备的**非空段**（on-start
/// 空轮只落时刻不产段，非空段只能是写信号触发的轮次发布的——删除信号路径
/// 即红）。S3 传输桥的真 HTTP 往返与阻塞等待在 OS 线程执行，不占测试主线程
/// （旧壳 wait_for_auto_round 同款现场形态）。
fn wait_for_auto_round(conn: Arc<std::sync::Mutex<rusqlite::Connection>>, space_id: &'static str) {
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
            let manifest: Option<ledger_sync_engine::ChannelManifest> = channel
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

/// 写后触发接线（接线半边）：启动竖井注入短时机拉起（含 on-start 空轮收尾
/// 等待，消除 on-start 与写触发的竞速——空轮只落时刻不产段，非空段只能是
/// 写信号轮次发布的）→ 域公开写入口记账 → 写信号 → 去抖合流 → 轮次发布。
#[test]
fn write_entry_enqueues_upload_via_scheduler() {
    let stub = test_support::s3::spawn_s3_stub(test_support::s3::S3StubConfig::new(
        test_support::s3::S3Addressing::PathStyle,
    ));
    let config = stub.channel_config("family");
    let dir = {
        let dir = seed::make_book_dir("sync-write-after");
        seed::create_empty_book_file(&dir);
        dir
    };

    // 预引导配置通道（配置不产 op——设备偏好留主库设置面，CONTEXT-sync
    // SyncDb 词条口径）。
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        configure_channel(&db, &config);
    }

    // 启动竖井（显式时机版）：引导 → 会话就位 → 接线（含 on-start 空轮 +
    // 调度线程，注入「短去抖 + 超长轮询」——接线断掉时不得被低频轮询救活，
    // issue #959）。
    nsh_app::app::set_book_dir(dir.clone());
    assert_eq!(
        nsh_app::app::run_startup_with_trigger_timings(TriggerTimings {
            poll_interval: Duration::from_secs(600),
            write_debounce: Duration::from_millis(50),
        }),
        nsh_app::app::StartupView::Ready,
        "引导成功呈现就绪态"
    );

    // 等 on-start 空轮收尾（落「上次成功同步时刻」即完成凭证）：wire 同时
    // 拉起打开即同步一次性轮次，它与下方写竞速——不先等它收尾，本机 op 可能
    // 被 on-start 轮次顺带发布，写信号路径红不了。
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let conn = db.read_conn.lock().unwrap();
            let stamped = settings::get::<Option<String>>(
                &conn,
                SettingKey::SyncLastSyncAt,
                None,
            )
            .unwrap()
            .is_some();
            drop(conn);
            if stamped {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "on-start 空轮未在限时内落上次同步时刻"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    // 经域公开写入口记账（写入口 → 行为编排 → record_local 全链在真实路径上，
    // 写信号随写事务投递）。
    {
        let db = nsh_app::boot::boot_ledger(&dir).unwrap();
        let account = seed::seed_cash_account(&db);
        let conn = db.conn.lock().unwrap();
        ledger_transaction::create(
            &conn,
            seed::expense_input(&account, &seed::test_date(0), 10_000, "触发接线的账"),
        )
        .expect("记账应成功");
    }

    let db = nsh_app::boot::boot_ledger(&dir).unwrap();
    wait_for_auto_round(Arc::clone(&db.conn), "family");
}
