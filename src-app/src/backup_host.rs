//! 自动备份调度宿主接线（issue #2078；ADR-0112 决策 5：下层定义注册点、上层
//! 注册实现、壳层启动时接线）。
//!
//! 旧壳把 AppHandle 状态（写句柄、两扇门、版本号）交给备份域门内
//! `start_scheduler` / `exit_fallback`（tauri-host 门，#2074）；NSH 进程零
//! tauri（GTK3/WebKitGTK 与 GTK4 同进程符号互踩），经备份域公开的**平台中立
//! 调度面**接线：[`ledger_backup::start_scheduler_with`]（收写句柄 + 版本号 +
//! 每轮业务可用性判定）与 [`ledger_backup::exit_fallback_with`]。调度节奏
//! （10 分钟轮询、单一 tick 双判定〔到期备份 + 定时计划追补〕）、日界门、产物
//! 命名与清理口径全部沿用 backup 域既有语义，本模块零新口径。
//!
//! 三个接线点（与旧壳 setup / `start_background_services` / `RunEvent::Exit`
//! 三面同位）：
//! - [`install_shell_side`]：应用根组件装载期（引擎线程，根 owner 永生）——
//!   设备偏好推送（备份目录镜像，旧壳 `DevicePreferenceSyncHost` 启动 immediate
//!   推送同位）+ 深路径发射器注入（`ledger_infra::events::init_deep_path_emitter`
//!   注册点，旧壳 `init_event_app` 同位）+ 失效信号泵（备份产物变更 pending
//!   → [`crate::signals`] 广播）；
//! - [`start_session_host`]：启动竖井成功分支（`run_startup`，`boot_ledger`
//!   成功且日志档位接管之后）——会话首次兜底补跑 + 调度线程挂载。旧壳拉起条件
//!   「`!locked && !boot_failed` 才拉起」的等价保留：NSH 立桩只引导明文库，
//!   引导失败即无会话、本函数不被调用（无锁定/失败占位形态，无解锁/重置恢复
//!   编排点——恢复宿主随迁移系列票开门时在同一编排点补拉起）；
//! - [`exit_fallback`]：进程退出兜底（bin 主函数在平台事件循环返回后调用）。
//!   **裁决留痕**：nsh-platform-linux 0.4.0 / nsh-core 0.4.0 无 `RunEvent::Exit`
//!   等价物（`AppOptions` 无退出回调字段，`run_app` 是闭合函数，窗口 destroy
//!   钩子〔`connect_destroy` / `AppView::stop`〕为平台内部实现、不对应用层暴露，
//!   亦无信号处理）；`run_app` 同步返回 = GTK 主循环结束 = 窗口关闭的正常退出
//!   路径，在其返回后调用即旧壳 `RunEvent::Exit` 的正常退出触发面（旧壳
//!   `RunEvent::Exit` 同样不覆盖 SIGKILL/崩溃；覆盖面一致，零新机制）。
//!
//! 每轮业务可用性判定：NSH 立桩注册恒真闭包——会话只在本票编排点（引导成功）
//! 存在，进程内无锁定/失败占位形态；加密支持随迁移系列票开门时换供自己的门
//! 判定，注册点形状不变（见 [`ledger_backup::BusinessAvailability`]）。
//!
//! 备份成功反馈：深路径执行点（连接层写入口提交点 / 调度线程）只持 `&Connection`，
//! 域内经 `emit_backups_changed_current` 发射；本模块注入的发射器把发射落成
//! 进程级 pending 计数（[`crate::signals::record_backups_pending`]，任意写线程
//! 可调），引擎线程泵消费计数后换 [`crate::signals::notify_backups_invalidated`]
//! 广播——不引入 AppHandle emit。pending 桥与 1s 泵是旧壳事件即时刷新的本壳
//! 对应形态：备份节奏 10 分钟 / 写触发，≤1s 反馈延迟不可感（AGENTS.md 体验取舍）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ledger_infra::db::DbState;
use nsh_core::prelude::*;

use crate::{app, backup_entry, settings_data, signals};

/// 会话首次兜底的待跑标志（App 装载期偏好推送认领机会、会话就绪后补跑——
/// 旧壳 `set_auto_backup_dir` 启动推送即跑，NSH 推送早于引导完成，跑点后移到
/// 会话就绪；认领时机与「每会话至多一次」语义逐字沿用域内
/// [`ledger_backup::PrefsState::claim_first_fallback`]）。
static FIRST_FALLBACK_PENDING: AtomicBool = AtomicBool::new(false);

/// 深路径发射器（infra [`ledger_infra::events::SignalEmitter`] 注册点的 NSH
/// 实现）：把 `ledger:backups-changed` 发射落成进程级 pending 计数。无场景态
/// （目标是无锁原子的进程级计数），headless 多场景串行下先注册者胜出、后续
/// 场景零动作，而计数桥接对全部场景持续生效——桥接与消费两侧任意场景顺序下
/// 语义不变（[`crate::signals::BACKUPS_PENDING`] 侧留痕）。
struct BackupsChangedRelay;

impl ledger_infra::events::SignalEmitter for BackupsChangedRelay {
    fn post(&self, event: &'static str) {
        if event == ledger_infra::events::BACKUPS_CHANGED {
            signals::record_backups_pending();
        }
    }
}

/// 设备偏好推送（备份目录镜像；旧壳 `set_auto_backup_dir` 命令语义逐字沿用）：
/// 空串视为未配置；镜像更新 + 「首次兜底」机会认领（每会话至多一次，空推送
/// 同样消费认领机会——旧壳启动 immediate 推送先于有效目录的同款行为）；
/// 认领成功且目录有效时置待跑标志，会话就绪后由 [`start_session_host`] 补跑。
///
/// 引擎线程调用（偏好读是引擎线程面，见 [`settings_data::pref_backup_dir`]）；
/// 设置卡目录选择 / 清除动作消费同一函数（变更推送，旧壳 watch immediate 同位
/// ——彼时认领机会已被启动推送消费，镜像更新即时生效）。
pub(crate) fn push_backup_dir(dir_pref: &str) {
    let trimmed = dir_pref.trim();
    let normalized = (!trimmed.is_empty()).then(|| trimmed.to_string());
    let prefs = ledger_backup::shared_prefs();
    prefs.set_dir(normalized.clone());
    let claim = prefs.claim_first_fallback();
    if normalized.is_none() || !claim {
        return;
    }
    FIRST_FALLBACK_PENDING.store(true, Ordering::SeqCst);
}

/// 会话首次兜底补跑（issue #125 既有语义：本会话首次提供有效目录时，若受管
/// 备份列表为空且开关开启立即备份一次）。后台独立线程执行（备份是数秒级阻塞
/// IO，不得阻塞启动竖井——旧壳启动推送走异步 IPC 命令同口径）；连接取用走
/// 门面限时等待写槽裸作业（与调度线程 / 退出兜底同款「取不到就放弃本轮」）；
/// 结果只记日志不上抛（旧壳同款，反馈面归设置卡状态重读）。
fn run_first_fallback_in_background(db: &DbState, dir: String) {
    let conn = db.write_handle();
    let version = backup_entry::APP_VERSION.to_owned();
    std::thread::spawn(move || {
        let outcome = conn.run_raw_blocking_within(
            "backup.first-fallback",
            ledger_backup::LOCK_TIMEOUT,
            move |conn| {
                let _ =
                    ledger_backup::run_first_backup(conn, Some(&dir), &version, chrono::Utc::now());
                Ok(())
            },
        );
        // 兜底失败只留日志：下一触发点（写时顺带检查 / 调度轮）按既有语义重试。
        if let ledger_infra::db::LockOutcome::Abandoned = outcome {
            tracing::warn!("首次兜底等待数据库连接超时，本轮放弃（下个触发点重试）");
        }
    });
}

/// 应用根组件装载期接线（引擎线程；根 owner 永生，`App` 组件体内单点调用）：
/// 偏好推送（目录镜像）+ 深路径发射器注入 + 失效信号泵。调用先于启动任务
/// 轮询（组件体同步执行先于 `spawn` 任务被拉起），目录镜像就位早于任何调度轮。
pub(crate) fn install_shell_side() {
    // 深路径发射器注入（进程级一次，先装者优先；实现无场景态，多场景串行
    // 测试下先注册者对全进程生效）。旧壳 `init_event_app` 的同位接线。
    ledger_infra::events::init_deep_path_emitter(Box::new(BackupsChangedRelay));
    // 失效信号泵：深路径发射先落 pending（任意写线程），本泵在引擎线程周期
    // 消费并换广播——广播触达设置卡的备份列表 / 自动备份状态重读（订阅接线
    // 在 `settings_data::backup_cards`）。任务挂根 owner（随进程存亡）。
    let pump = set_interval(Duration::from_secs(1), || {
        if signals::take_backups_pending() {
            signals::notify_backups_invalidated();
        }
    });
    effect(move || {
        let _held_for_app_lifetime: &Spawned<()> = &pump;
    });
    // 设备偏好推送（旧壳 DevicePreferenceSyncHost 启动 immediate 推送同位）。
    push_backup_dir(&settings_data::pref_backup_dir());
}

/// 会话宿主接线（启动竖井成功分支；`run_startup` 内 `logging::apply_persisted`
/// 之后单点调用）：首次兜底补跑 + 调度线程挂载。删除本调用 → 自动备份调度
/// 失宿主（首次兜底不发生、轮询不启动）→ headless 首次兜底旅程与源码扫描
/// 守门双红。
pub(crate) fn start_session_host(db: &DbState) {
    // 会话首次兜底（认领机会在装载期推送时裁定；待跑标志在此消费）。
    if FIRST_FALLBACK_PENDING.swap(false, Ordering::SeqCst)
        && let Some(dir) = ledger_backup::shared_prefs().snapshot_dir()
    {
        run_first_fallback_in_background(db, dir);
    }
    // 调度线程挂载（域内单次拉起守卫，重引导/重复调用幂等；每轮业务可用性
    // 判定 = 恒真：NSH 立桩会话即业务可用，见模块文档）。轮询同轮承载定时
    // 计划追补（追补实现在 [`crate::wiring`] 对装，镜像默认关时空转）。
    ledger_backup::start_scheduler_with(
        db.write_handle(),
        backup_entry::APP_VERSION.to_owned(),
        Arc::new(|| true),
    );
}

/// 进程退出兜底（bin 主函数在平台事件循环返回后调用；触发面裁决见模块文档）：
/// 退出前若脏且当天尚未自动备份过则补一次备份（日界门约束，issue #386）。
/// 会话缺失（启动失败进程）零动作——旧壳锁定/失败态的退出兜底等同形（门检
/// 由会话存在性承载）。
pub fn exit_fallback() {
    let db = app::db_slot().clone();
    let Some(db) = db.as_ref() else {
        return;
    };
    ledger_backup::exit_fallback_with(&db.write_handle(), backup_entry::APP_VERSION);
}

/// headless 多场景串行测试的复位接缝（[`app::reset_book_assembly`] 消费；
/// 生产 bin 不消费）：目录镜像置空、首次兜底认领态与待跑标志复位——上一场景
/// 的进程级单例态不得泄入下一场景（镜像残留会让下一场景的自动备份写进上一
/// 场景的目录）。
#[doc(hidden)]
pub(crate) fn reset_shell_side_for_tests() {
    ledger_backup::shared_prefs().set_dir(None);
    ledger_backup::shared_prefs().reset_first_fallback_claim_for_tests();
    FIRST_FALLBACK_PENDING.store(false, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    //! NSH 编排点源码扫描守门（#2078；#959 / #961 形态，旧壳
    //! `check-background-services.ts` ⑤ BOOT_WIRING 同哲学）：三面接线点的
    //! 生产调用不被测试直接执行的部分（bin 退出兜底不编译进测试、启动竖井
    //! 调用点在多场景下不可行为区分），删接线不会让行为断言变红——以扫描
    //! 守门补位；行为面（首次兜底产物在盘）由 headless 旅程另钉
    //! （tests/app_headless.rs）。include_str! 锚点 = 本 crate 源文件，接线点
    //! 改名 / 搬迁后同步更新本测。

    /// 扫描源文件含标识符（字面量已随 include_str! 原样文本；按标识符存在性
    /// 断言，与旧壳 BOOT_WIRING 同形态——调用点折行不改判）。
    fn source_contains(source: &str, token: &str) -> bool {
        source.contains(token)
    }

    #[test]
    fn shell_side_wiring_is_mounted_in_app_body() {
        let app_rs = include_str!("app.rs");
        assert!(
            source_contains(app_rs, "backup_host::install_shell_side()"),
            "App 根组件体应装载自动备份宿主的壳侧接线（偏好推送 + 深路径发射器 + 信号泵）——删除即本测红"
        );
    }

    #[test]
    fn session_host_wiring_is_mounted_in_run_startup() {
        let app_rs = include_str!("app.rs");
        assert!(
            source_contains(app_rs, "backup_host::start_session_host(&db)"),
            "run_startup 成功分支应挂载自动备份宿主（首次兜底 + 调度线程）——删除即本测红"
        );
    }

    #[test]
    fn exit_fallback_wiring_is_mounted_in_bin_main() {
        let main_rs = include_str!("main.rs");
        assert!(
            source_contains(main_rs, "backup_host::exit_fallback()"),
            "bin 主函数应在平台事件循环返回后调用退出兜底——删除即本测红（bin 不编译进测试目标，扫描是唯一守门）"
        );
    }

    #[test]
    fn backup_domain_seams_are_wired_in_boot_wiring() {
        let wiring_rs = include_str!("wiring.rs");
        for token in [
            "ledger_backup::install_after_commit_hook()",
            "register_after_occurrence_hook",
            "ledger_backup::occurrence_dirty_hook",
            "register_catch_up_hook",
            "auto_run::catch_up_hook",
            "register_lock_hold_budget",
        ] {
            assert!(
                source_contains(wiring_rs, token),
                "域栈引导应接线 {token}——删除即本测红（置脏/追补/预算接线缺失是生产静默丢副作用）"
            );
        }
    }
}
