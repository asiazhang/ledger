//! 多端同步触发宿主（issue #2080 / ADR-0091 决策 9、ADR-0098）：sync-engine
//! 触发编排的非 tauri 宿主面接线——旧壳经 tauri-host 门化面（AppHandle）拉起，
//! NSH 经域的平台中立宿主面 [`ledger_sync_engine::TriggerHost`] 接线，触发
//! 闭集（打开即同步 / 桌面低频轮询 / 写后去抖即时同步 / 恢复入口）与旧壳
//! 同一段域编排，零新口径。
//!
//! AppHandle 三件用途逐件替换（对应关系）：
//! - **state 取连接与两扇门**：会话句柄 [`crate::app::db_slot`] 的写连接 +
//!   恒开门态（NSH 立桩只开明文库——`boot_ledger` 对密文库报
//!   `boot.encrypted-unsupported`，锁定态不存在；接线只在 [`crate::app::run_startup`]
//!   成功分支，启动失败占位不产生会话）——编排单点的门检仍在域内一处；
//! - **事件投递**：域回调 `on_round_applied(applied > 0)` → [`ROUND_TX`] 通道
//!   投递 → 引擎线程排空任务广播进程内失效信号（[`crate::signals`]；订阅者表
//!   住引擎线程 thread_local，调度线程只投递不广播——线程契约同写路径
//!   「重活在后台线程，回到引擎线程才广播」）；
//! - **后台执行**：域侧 `std::thread` 调度线程（与旧壳同形，NSH 不另设执行器）。
//!
//! 接线纪律（ADR-0112 决策 5：下层定义注册点、上层注册实现、壳层启动时接线）：
//! 域内两个内部触发入口（`start_sync_scheduler_with_host` /
//! `sync_on_start_with_host`）域外不得直呼——NSH 生产路径只经
//! [`wire_sync_triggers_with`] → 域编排单点 `start_triggers_with_host`（分平台门
//! 收在域内一处，旧壳 `sync_trigger_guard` 同款不变量）；本模块源码扫描守门
//! 钉住单向形状（下方 cfg(test)）。「写后即时同步」的响应闭包（域
//! `install_after_write_hook`）随 [`crate::wiring::install_domain_seams`] 接线。
//!
//! 测试接缝：[`wire_sync_triggers_with`] 显式收 [`TriggerTimings`]（issue #959
//! 显式参数接缝先例：生产传默认值，触发接线测试注入「短去抖 + 超长轮询」）。

use std::sync::{Arc, Mutex, MutexGuard, mpsc};

use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::encryption::EncryptionGate;
use ledger_sync_engine::{TriggerHost, TriggerTimings};
use nsh_core::prelude::*;

use crate::signals;

/// 轮次失效信号的投递通道（进程级单例）：域回调在调度线程投递，引擎线程
/// 排空任务消费。`Mutex<Option<_>>` 而非 `OnceLock`：headless 多场景串行测试
/// 需可复位（上一场景的排空任务随引擎撤销，其接收端不得响应下一场景的投递
/// ——复位点在 [`reset_for_tests`]，与失效信号订阅者同批复位）。
static ROUND_TX: Mutex<Option<mpsc::Sender<()>>> = Mutex::new(None);

fn round_tx_slot() -> MutexGuard<'static, Option<mpsc::Sender<()>>> {
    ROUND_TX.lock().unwrap_or_else(|p| p.into_inner())
}

/// 域回调的投递侧（调度线程调用；`applied > 0` 即本轮应用了外来 op）：通道
/// 未装（排空任务缺席，如直接经域入口跑轮次的测试）时投递零动作——自动轮次
/// 对壳层信号面无感，与旧壳「发射器缺席静默忽略」同口径。
fn on_round_applied(applied: bool) {
    if applied && let Some(tx) = round_tx_slot().clone() {
        let _ = tx.send(());
    }
}

/// 装载失效广播的引擎线程排空任务（App 根组件体调用一次；根 owner 永生，
/// 任务随根存活）：建通道 + `spawn` 排空循环。重复装载零动作（先装者优先；
/// 测试经 [`reset_for_tests`] 复位后重装）。
///
/// 线程契约（写路径同款）：接收端的阻塞 `recv` 在后台线程等待（不占引擎
/// 线程），`blocking` 完成回到引擎线程才广播——订阅者表住引擎线程
/// thread_local，跨线程直调是零动作而非广播。接收端随闭包进出（`mpsc::Receiver`
/// 非 `Copy`），通道断裂（发送端全灭：测试复位）即排空任务结束。
pub(crate) fn install_invalidation_drain() {
    let mut slot = round_tx_slot();
    if slot.is_some() {
        return;
    }
    let (tx, rx) = mpsc::channel::<()>();
    *slot = Some(tx);
    drop(slot);
    spawn(async move {
        let mut rx = Some(rx);
        while let Some(current) = rx.take() {
            let Ok((message, returned)) = blocking(move || (current.recv(), current)).await else {
                // 排空闭包异常退出（不 panic 引擎线程的既有口径）：广播通道退役。
                return;
            };
            rx = Some(returned);
            if message.is_err() {
                return; // 发送端全灭（测试复位）：排空任务随通道退场
            }
            // 轮次应用外来 op = 账本数据变化（对端写入落库）：三族失效信号
            // 与手动同步完成后的广播同闭集（settings_data sync_now_task 同款），
            // 否则「同步了但界面不动」。
            signals::notify_transactions_invalidated();
            signals::notify_budgets_invalidated();
            signals::notify_investments_invalidated();
        }
    });
}

/// 接线单点：装配平台中立宿主面并经域编排单点
/// `start_triggers_with_host` 拉起（分平台门收在域内一处）。会话缺失即静默
/// 返回——接线只在引导成功分支（占位连接不是业务库，与旧壳「锁定/失败不拉起」
/// 同口径）。唯一生产调用方是 [`crate::app::run_startup_with_trigger_timings`]
/// 成功分支（源码扫描守门钉住）；生产时机走默认常量，触发接线测试注入短时机
/// 经启动竖井的显式参数（issue #959）。
///
/// 两扇门以恒开形态注入：NSH 立桩只开明文库（锁定态不存在）、接线只在引导
/// 成功分支（启动失败占位不产生会话）——编排循环的门检语义不变，宿主按自身
/// 状态机供给门态（NSH 引入密文库/启动失败恢复路径时换真实门态）。
pub(crate) fn wire_sync_triggers_with(timings: TriggerTimings) {
    let Some(db) = crate::app::db_slot().clone() else {
        return;
    };
    let host = Arc::new(TriggerHost {
        conn: Arc::clone(&db.conn),
        gate: EncryptionGate::new(false),
        boot_gate: BootFailureGate::new(),
        on_round_applied: Arc::new(on_round_applied),
    });
    ledger_sync_engine::start_triggers_with_host(&host, timings);
}

/// 复位投递通道（headless 多场景串行测试的复位接缝，生产 bin 不消费）：上一
/// 场景的排空任务随其引擎撤销，通道一并退役——下一场景的 App 重装 fresh
/// 通道与排空任务。与 [`crate::app::reset_book_assembly`] 同批复位点。
#[doc(hidden)]
pub(crate) fn reset_for_tests() {
    *round_tx_slot() = None;
}

/// 源码扫描守门（旧壳 `sync_trigger_guard` 同款格式，词法器具共享根包
/// `test_support::scan` 经 `tests/support/s3_stub.rs` facade 载入）：NSH 引入
/// 了壳内接线编排点，按既有格式登记——分平台门与触发时机编排只许经域编排
/// 单点，壳层接线点与广播排空装载点各恰一处。
#[cfg(test)]
mod guard_tests {
    use super::super::test_support::scan::{mask_non_code, matching_brace_end};
    use std::path::Path;

    /// `src/**` 下全部 .rs 的（仓库相对路径, 源文本），按路径排序（输出确定）。
    fn all_rust_sources() -> Vec<(String, String)> {
        fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
            let mut entries: Vec<_> = std::fs::read_dir(dir)
                .unwrap_or_else(|e| panic!("扫描目录不可读 {dir:?}: {e}"))
                .flatten()
                .collect();
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, root, out);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let rel = path
                        .strip_prefix(root)
                        .expect("源码在 manifest 内")
                        .to_string_lossy()
                        .into_owned();
                    out.push((rel, std::fs::read_to_string(&path).expect("源码应可读")));
                }
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut out = Vec::new();
        walk(&root.join("src"), root, &mut out);
        out
    }

    /// 生产文本：掩码注释与字符串后，逐个剔除 `#[cfg(test)]` 附属的模块/函数
    /// 体——测试模块内合法直呼触发入口不参与守门（旧壳 `sync_trigger_guard`
    /// 同款；声明式与内联体两种形态都剔，花括号配对在掩码文本上做）。
    fn production_text(src: &str) -> String {
        let mut masked = mask_non_code(src);
        while let Some(anchor) = masked.find("#[cfg(test)]") {
            let rest = &masked[anchor..];
            let declaration_end = rest.find(';').map(|p| anchor + p);
            let brace = rest.find('{').map(|p| anchor + p);
            let end = match (declaration_end, brace) {
                (Some(semi), None) => semi + 1,
                (_, Some(brace)) => matching_brace_end(&masked, brace).unwrap_or(masked.len()),
                (None, None) => rest.find('\n').map_or(masked.len(), |p| anchor + p),
            };
            let blank = " ".repeat(end - anchor);
            masked.replace_range(anchor..end, &blank);
        }
        masked
    }

    /// 触发接线形状（旧壳 `sync_triggers_start_from_every_business_surface_via_single_entry`
    /// 的 NSH 形态）：① 域内两个内部触发入口域外零直呼（分平台门与触发时机
    /// 编排只在域编排单点）；② 域编排单点 `start_triggers_with_host` 的生产
    /// 调用方恰为壳内接线单点 `sync_triggers.rs`；③ 接线入口与广播排空装载的
    /// 生产调用点恰为启动竖井成功分支（`app.rs`）——删任一接线即在此红（行为
    /// 半边由触发接线测试与 headless 旅程钉住，本守门钉源码形状半边）。
    #[test]
    fn sync_triggers_wire_from_run_startup_via_single_entry() {
        let sources = all_rust_sources();

        for (path, src) in &sources {
            let text = production_text(src);
            for entry in [
                "start_sync_scheduler_with_host(",
                "sync_on_start_with_host(",
            ] {
                assert!(
                    !text.contains(entry),
                    "{path} 直呼触发内部入口 `{entry}`——分平台门与触发时机编排只收在 \
                     域编排单点 start_triggers_with_host（ADR-0098 决策 4），壳层只经 \
                     wire_sync_triggers_with 接线单点拉起（issue #2080）"
                );
            }
        }

        let mut callers: Vec<&str> = sources
            .iter()
            .filter(|(_, src)| production_text(src).contains("start_triggers_with_host("))
            .map(|(path, _)| path.as_str())
            .collect();
        callers.sort_unstable();
        assert_eq!(
            callers,
            ["src/sync_triggers.rs"],
            "域编排单点调用方漂移——生产调用只能住在壳内接线单点 \
             wire_sync_triggers_with（issue #2080：run_startup 成功分支唯一接线点）"
        );

        for (entry, owner) in [
            ("sync_triggers::wire_sync_triggers_with(", "同步触发接线"),
            (
                "sync_triggers::install_invalidation_drain(",
                "失效广播排空装载",
            ),
        ] {
            let mut sites: Vec<&str> = sources
                .iter()
                .filter(|(_, src)| production_text(src).contains(entry))
                .map(|(path, _)| path.as_str())
                .collect();
            sites.sort_unstable();
            assert_eq!(
                sites,
                ["src/app.rs"],
                "{owner}调用点漂移——唯一生产调用方是启动竖井成功分支（run_startup）与 \
                 App 根组件（广播排空），删接线即红（issue #2080）"
            );
        }
    }
}

/// 打开即同步域入口测试（旧壳 tests/commands/sync_trigger.rs 的域入口半边在
/// NSH 侧重生，票面测试决策）：真 S3 桩 + 产品开库入口 + 域公开写入口，断言
/// 对准用户可观察结果——「上次成功同步时刻」落库 + 通道 manifest 上出现本机
/// 设备的非空段（另一端可见的本机段），不对准线程或函数调用形状
///（CONTEXT-testing〈断言强度〉）。壳层接线半边（run_startup 接线点 + 写后
/// 去抖 + 失效广播）归独立测试二进制：tests/sync_trigger.rs（旅程）与
/// tests/sync_trigger_write.rs（写后接线，短时机注入）——调度线程单次拉起
/// 守卫是进程级先到先得，各二进制唯一场景即首个 wire 调用方（旧壳分进程
/// 同因）。低频轮询行为断言归域权威既有覆盖（trigger/tests.rs），NSH 面不
/// 重复。
#[cfg(test)]
mod trigger_tests {
    use crate::app::reset_book_assembly;
    use crate::boot::boot_ledger;
    use crate::test_support::s3::{S3Addressing, S3Stub, S3StubConfig, spawn_s3_stub};
    use crate::tests::{
        create_empty_book_file, expense_input, scratch_book_dir, seed_cash_account, test_date,
    };
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use ledger_infra::db::boot::BootFailureGate;
    use ledger_infra::db::encryption::EncryptionGate;
    use ledger_infra::settings::{self, SettingKey};
    use ledger_sync_engine::{TriggerHost, build_channel, configured_channel};

    /// 场景通道桩（明文 http 端点——保存门只收 https，配置经设置域单点直落，
    /// 旧壳 configure_channel 同型）。
    fn spawn_sync_stub() -> S3Stub {
        spawn_s3_stub(S3StubConfig::new(S3Addressing::PathStyle))
    }

    /// 经设置域单点直落通道配置（不经保存命令：桩是明文 http 端点，保存门只
    /// 收 https——保存门的拒绝路径归 headless 同步卡旅程既有覆盖）。
    fn configure_channel(db: &ledger_infra::db::DbState, stub: &S3Stub) {
        let conn = db.conn.lock().unwrap();
        settings::set(
            &conn,
            SettingKey::SyncChannelConfig,
            &stub.channel_config("family"),
        )
        .expect("通道配置应落库");
    }

    /// 等待自动轮次的用户可观察结果并断言（限时轮询，OS 线程执行）：①「上次
    /// 成功同步时刻」落库；②通道 manifest 上出现本机设备的**非空段**（写后
    /// 触发用例先等 on-start 空轮收尾再写，非空段只能是写信号触发的轮次发布
    /// 的——删除信号路径即红）。整段移出测试线程：S3 传输桥的真 HTTP 往返与
    /// 阻塞等待不得占用测试主线程（旧壳 wait_for_auto_round 同款现场形态）。
    fn wait_for_auto_round(
        conn: Arc<std::sync::Mutex<rusqlite::Connection>>,
        space_id: &'static str,
    ) {
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

    /// 打开即同步接线（issue #863 验收「打开应用即自动同步」的域入口半边）：
    /// 业务可用起点经平台中立入口 `sync_on_start_with_host` 拉起一次性后台
    /// 轮次，把既有本机 op 发布上通道。`run_startup` → 接线点的壳侧行为半边
    /// 由 headless 旅程（tests/app_headless.rs 对端段上屏）钉住；调用点存在性
    /// 由源码扫描守门（上方 guard_tests）钉住——删任一环即红（ADR-0087）。
    #[test]
    fn sync_on_start_publishes_local_ops_to_channel() {
        let _assembly = crate::app::test_assembly_guard();
        reset_book_assembly();
        let stub = spawn_sync_stub();
        let dir = scratch_book_dir("sync-onstart");
        create_empty_book_file(&dir);

        // 铺垫：通道配置 + 一笔可发布本机 op（垫数先于拉起——on-start 轮次
        // 发布既有本机 op，旧壳同序）。本测试不拉调度线程：写信号无接收端，
        // 零动作——「写路径对同步域无感」的另一形态顺带在位。
        let db = boot_ledger(&dir).expect("开库应成功");
        configure_channel(&db, &stub);
        let account = seed_cash_account(&db);
        {
            let conn = db.conn.lock().unwrap();
            ledger_transaction::create(
                &conn,
                expense_input(&account, &test_date(0), 2_500, "打开即同步的一笔"),
            )
            .unwrap();
        }

        // 平台中立入口直拉（不经 wire：接线单点会连调度线程一起拉起，其单次
        // 拉起守卫是进程级单例，归写后触发用例独占）。
        let host = Arc::new(TriggerHost {
            conn: Arc::clone(&db.conn),
            gate: EncryptionGate::new(false),
            boot_gate: BootFailureGate::new(),
            on_round_applied: Arc::new(|_| {}),
        });
        ledger_sync_engine::sync_on_start_with_host(&host);

        wait_for_auto_round(Arc::clone(&db.conn), "family");
    }
}
