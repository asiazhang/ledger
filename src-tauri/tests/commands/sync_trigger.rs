//! 同步触发时机接线集成测试（issue #959 / #863 验收 / ADR-0098）。
//!
//! #863 的「打开应用即自动同步；写 op 后即时入队上传」此前只有域单测（决策
//! 半边：去抖合流、未配置零动作）与「调度未拉起」的反例——**接线半边**（壳层
//! 写入口 → `record_local` 写信号 → 调度线程去抖合流 → 轮次发布；业务可用起点
//! → `sync_on_start` 后台轮次）零断言，删掉接线测试仍全绿。本文件用 mock 应用
//! 加真实 S3 桩（与 [`crate::sync_channel`] 同现场形态）钉接线半边：断言
//! 对准用户可观察结果（通道上出现本机段、上次同步时刻落库），不对准线程或
//! 函数调用形状（CONTEXT-testing〈断言强度〉；测试三层权威见 ADR-0087）。
//!
//! 时机参数接缝：去抖窗口与轮询周期是生产常量，测试经显式参数
//!（`TriggerTimings`，`ChannelOptions` 同款接缝，不加测试后门）注入「短去抖 +
//! 超长轮询」——写后触发断言只等去抖窗；接线断掉（写信号删除）时低频轮询
//! 远水救不了近火，测试必须红。
//!
//! 分平台门（`#[cfg(desktop)]` 收在 `start_triggers` 单点、三业务可用起点统一
//! 调用）是源码形状事实，由域内源码扫描守门钉住（`sync_engine::trigger::tests`，
//! `signals_cross_check` 先例），不在本文件。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use tauri::Manager;

use tauri_app_lib::commands::{accounts, transactions};
use tauri_app_lib::db::boot::BootFailureGate;
use tauri_app_lib::db::encryption::EncryptionGate;
use tauri_app_lib::db::{self, DbState};
use tauri_app_lib::settings::{self, SettingKey};
use tauri_app_lib::sync_engine::{
    ChannelManifest, TriggerTimings, build_channel, configured_channel, start_sync_scheduler_with,
    sync_on_start,
};

use crate::isolation::isolate_home;
use crate::sync_channel::{configure_channel, expense_input, fresh_app, spawn_sync_stub};

/// 调度线程现场（消费两扇门做锁定/失败空转判定）的设备应用：mock 应用 + 独立
/// 临时目录文件库 + 引导登记态 + 两扇门（生产由 setup 首先登记，此处同型补齐；
/// 与 [`crate::sync_channel::device_app`] 的差别仅在门）。
fn trigger_device_app(tag: &str) -> (tauri::AppHandle<tauri::test::MockRuntime>, PathBuf) {
    let (app, dir) = fresh_app(tag);
    app.manage(db::open_db_in(&dir).unwrap());
    app.manage(EncryptionGate::new(false));
    app.manage(BootFailureGate::new());
    (app.handle().clone(), dir)
}

/// 经壳层公开命令铺垫一笔可发布的本地产出（建户 + 记账 = 写入口 → 行为编排 →
/// `record_local` 全链在真实路径上，写信号随写事务投递）。
async fn seed_own_ops(app: &tauri::AppHandle<tauri::test::MockRuntime>) {
    let acc_id = accounts::create_account(
        app.state(),
        app.clone(),
        tauri_app_lib::accounts::AccountInput {
            name: "现金".into(),
            kind: tauri_app_lib::accounts::AccountType::Cash,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(0),
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

/// 等待自动轮次的用户可观察结果并断言（限时轮询）：①「上次成功同步时刻」
/// 落库；②通道 manifest 上出现本机设备的段（另一端可见的本机段）。
///
/// 整段移出异步上下文（OS 线程执行）：reqwest 阻塞客户端（构库 + 传输读）在
/// tokio 运行时内构造/析构会 panic——与产品侧把阻塞 IO 放进阻塞线程池同一
/// 语义（ADR-0069 / [`crate::sync_channel`] 同款现场形态）。
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

/// 写后触发接线（issue #863 验收「写 op 后即时入队上传」的接线半边）：调度
/// 线程在位（注入短去抖）时，经壳层写入口记一笔 → 去抖合流 → 轮次发布 →
/// 通道上出现本机段 + 上次同步时刻落库。删掉 `record_local` 的写信号投递，
/// 本测试红（轮询周期超长，救不了场）。
#[tokio::test]
async fn write_entry_enqueues_upload_via_scheduler() {
    isolate_home();
    let stub = spawn_sync_stub();
    let (app, _dir) = trigger_device_app("write-after");
    configure_channel(&app, &stub);

    // 注入时机：短去抖（写后只等静默窗）+ 超长轮询（远超测试时长——接线断掉
    // 时不得被低频轮询「救活」，信号删除必须红）。本测试是本测试二进制内唯一
    // 拉起调度线程的测试（单次拉起守卫与写信号通道是进程级单例）：先行的拉起
    // 会让本处注入失效。低频轮询行为断言在独立二进制（tests/sync_trigger_poll.rs）。
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

/// 打开即同步接线（issue #863 验收「打开应用即自动同步」）：业务可用起点调
/// `sync_on_start` 后，一次性后台轮次把既有本机 op 发布上通道。lib.rs 等起点
/// 的调用点存在性由域内源码扫描守门钉住（trigger::tests）。
#[tokio::test]
async fn sync_on_start_publishes_local_ops_to_channel() {
    isolate_home();
    let stub = spawn_sync_stub();
    let (app, _dir) = trigger_device_app("on-start");
    configure_channel(&app, &stub);

    // 先经壳层写入口铺垫一笔（有可发布内容）。本测试不拉调度线程：写信号无
    // 接收端，零动作——「写路径对同步域无感」的另一形态顺带在位。
    seed_own_ops(&app).await;

    sync_on_start(&app);

    let conn = app.state::<DbState>().conn.clone();
    wait_for_auto_round(conn, "family");
}
