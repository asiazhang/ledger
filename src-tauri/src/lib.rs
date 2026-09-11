// 测试整体豁免（ADR-0060）：clippy 六件套 deny 仅约束生产路径；单元测试目标
// （含 src/** 内 #[cfg(test)] 模块）经 crate 根 cfg(test) 整体放行，生产构建零放宽。
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

pub mod accounts;
pub mod api_server;
pub mod backup;
pub mod budget;
pub mod categories;
pub mod commands;
pub mod currencies;
pub mod dashboard;
pub mod investment;
pub mod item;
pub mod merchants;
pub mod physical_asset;
pub mod policy;
pub mod reports;
pub mod scheduled_transactions;
pub mod sync;
pub mod sync_engine;
// 信号守门测试（signals_cross_check，ADR-0044 决策 3 修订 / ADR-0073 决策 5）：
// 写路径接线源码扫描核对，仅测试可见。
#[cfg(test)]
mod signals_cross_check;
// 统一测试数据库工厂与共享断言库（ADR-0084，issue #751）：建库/种子/对拍断言
// 单一入口，域测试与外部集成测试共用；仅测试消费，生产路径不得消费
//（C 类豁免声明，ADR-0060）。
#[doc(hidden)]
pub mod test_support;
pub mod transaction;

use tauri::Manager;
use tauri::ipc::Invoke;
// 基础设施全量归位（spec #1086 / issue #1088）：数据库、错误、设置、文件工具、
// 日志、事件、信号、闭集与壳层统一读写入口迁入 `ledger-infra`，根包以再导出
// 形态保留原引用路径——域与壳层的 `crate::db::…` / `crate::error::…` 等调用点
// 零改动即可编译（expand 形态）。
pub use ledger_infra::{
    closed_set, db, error, events, fs_util, logger, read_entry, settings, signals, write_entry,
};
// 测试支持器具随基础设施归位（issue #1088）：类型身份要求与 `events::SignalEmitter`
// 同 crate，根包原路径 `crate::test_utils` / `tauri_app_lib::test_utils` 经再导出保持。
#[doc(hidden)]
pub use ledger_infra::test_utils;
// IPC 载荷脱敏（issue #1087 首位成员）：调用面保持原函数名。
use ledger_infra::redact::redact_passphrase_payload;
// 对话框兜底仅桌面参与（issue #558 / ADR-0074 决策 6）：移动端启动期 DB 初始化
// 二次失败改走记日志后带错误退出，不引入主线程阻塞对话框（见 run() 的二次失败兜底）。
#[cfg(desktop)]
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

use crate::commands::boot::{boot_sequence, recover_boot_failure};
use crate::db::boot::BootFailureGate;
use crate::db::encryption::EncryptionGate;

// 命令注册单一来源（ADR-0047）：由 build.rs 扫描 #[tauri::command] 注解生成、
// include! 进本 crate；命令注册零手工清单，新增/删除命令只改命令域文件本身。
include!(concat!(env!("OUT_DIR"), "/commands_registry.rs"));

/// 锁定期间放行的 IPC 命令白名单（issue #570 / #574 / ADR-0075 决策 5）：解锁屏
/// 启动期所需的最小面（启动状态查询、解锁、凭缓存解锁、平台能力查询、忘记口令重置
/// #573；get_boot_status 是启动三态探测的统一入口，锁定与就绪态都要可达）。
/// #603 起解锁屏常驻「从备份文件恢复」入口，恢复通道最小命令面随白名单放行
/// （备份元数据校验、恢复执行、恢复成功后自动重启；get_encryption_status 既是
/// 解锁屏既有面也供恢复前的当前模式探测）——恢复命令对「无已打开库连接」可用
/// （issue #601 前置修复），锁定期间的占位连接同形；其余命令在解锁前一律拒绝
/// （解锁先于一切业务读写）。
const LOCKED_ALLOWED_COMMANDS: &[&str] = &[
    "get_boot_status",
    "get_encryption_status",
    "unlock_encryption",
    "unlock_with_remembered_passphrase",
    "get_remember_passphrase_support",
    "reset_after_forgotten_passphrase",
    "get_backup_meta",
    "restore_backup",
    "restart_app",
];

/// 启动失败期间放行的 IPC 命令白名单（issue #601 / #602 / ADR-0075 决策 5 修订）：
/// 失败恢复屏所需的最小面（启动状态查询、重置为空库；#602 备份恢复通道：
/// 备份元数据校验、当前模式探测、恢复执行、恢复成功后自动重启）；其余命令
/// 一律拒绝——占位连接不是业务库，任何业务读写都不得触达。
const BOOT_FAILURE_ALLOWED_COMMANDS: &[&str] = &[
    "get_boot_status",
    "reset_after_startup_failure",
    "get_backup_meta",
    "get_encryption_status",
    "restore_backup",
    "restart_app",
];

/// 业务可用起点的**后台服务编排单一入口**（issue #961）：自动备份调度
/// （[`backup::start_scheduler`]，轮询同轮承载定时计划追补）与多端同步触发
/// （[`sync_engine::start_triggers`]，分平台门收在域内一处，ADR-0098 决策 4）
/// 必须在每个业务可用起点成对拉起——两个独立调用无机制保证成对，#863 会话
/// 已由同一根因造成两次真实缺陷（分平台门漂移、`restart_app` 落 Ready 漏接
/// 同步触发），且「缺失一个调用」不会让任何断言变红。全部业务可用起点只调
/// 本函数：
/// ① `lib.rs` setup 就绪（锁定/失败不拉，守卫在调用方）；
/// ② `resume_business_surface`（解锁 / 忘记口令重置 / 启动失败重置共用）；
/// ③ `restart_app` 原位重引导落 `Ready`（不经 setup 也不经解锁路径）。
/// 成对性由本函数与 `scripts/check-background-services.ts` 文本守门共同保证：
/// 两个域入口的生产调用只允许出现在本函数体内，其余位置命中即红。
/// 各调度自持单次拉起守卫，原位重引导重复调用幂等（ADR-0080）。
pub(crate) fn start_background_services(app: &tauri::AppHandle) {
    backup::start_scheduler(app);
    sync_engine::start_triggers(app);
}

fn try_init_database(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("开始初始化数据库");
    // 启动引导序列与重启命令（原位重引导）同一段（commands::boot::boot_sequence）：
    // DataLocation 引导 → 生效库文件处置 → 连接登记/换入 + 两扇门翻转，
    // 「重启后状态 = 新进程启动状态」恒成立（issue #644 / ADR-0080）。
    match boot_sequence(app) {
        Ok(phase) => {
            tracing::info!(phase = phase.as_str(), "启动引导序列完成");
            Ok(())
        }
        Err(e) => {
            // 启动失败前端接管（issue #601 / ADR-0075 决策 5 修订）：不再弹原生
            // 「重置/退出」对话框、不再退出——失败状态经 BootFailureGate 暴露给
            // 前端，由启动失败恢复屏承担恢复通道（重置为空库 + 从备份文件恢复，
            // 见 `commands::boot`）。此处 Err 仅在失败登记本身失败（连占位内存
            // 库都建不起）时上抛，由 run() 的二次失败兜底 fail loud。
            recover_boot_failure(app, &e)?;
            Ok(())
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 两扇进程级门（issue #570 / #601）：实例在装配期创建，同一份分别供
    // IPC 门禁（invoke wrapper 捕获）与启动编排（setup 登记）、HTTP 门禁消费。
    let gate = EncryptionGate::new(false);
    let ipc_gate = gate.clone();
    let boot_gate = BootFailureGate::new();
    let ipc_boot_gate = boot_gate.clone();
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init());
    // 窗口状态插件仅桌面参与（issue #558 / ADR-0074 决策 6）：tauri-plugin-window-state
    // 在 Android 目标下整体不参与编译，Cargo.toml 依赖与注册处均按目标平台门禁
    // （缺一即 Android 编译失败）；桌面行为零变化。
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_window_state::Builder::default().build());
    // B 类豁免（ADR-0060）：启动装配失败即无法运行——Tauri 构建失败 fail loud 退出进程。
    #[allow(clippy::expect_used)]
    builder
        .setup(move |app| {
            logger::init(app.handle());
            // 写路径副作用接缝接线（spec #1086 / issue #1088）：连接层写入口提交点的
            // 后置动作（置脏 + 写时顺带到期检查，ADR-0032）由备份域提供实现、壳层
            // 启动时注册——基础设施 crate 不再反向依赖业务域；注册先于任何建库/写库。
            backup::install_after_commit_hook();
            // 两扇进程级门先登记（boot_sequence 与 IPC/HTTP 门禁共同消费；实例
            // 由 run() 创建，同一份供 invoke wrapper 共享）：加密锁定门 + 启动
            // 失败门（issue #601）。
            app.manage(gate.clone());
            app.manage(boot_gate.clone());
            try_init_database(app.handle()).map_err(|e| {
                // 二次失败兜底（登记失败状态也失败，如连占位内存库都打不开）：
                // 进程无法运行，fail loud 退出（B 类豁免，ADR-0060）。
                // 分平台（issue #558 / ADR-0074 决策 6）：桌面保留模态弹窗告知用户；
                // 移动端阻塞式对话框会死锁主线程，记日志后带错误退出。
                #[cfg(desktop)]
                app.dialog()
                    .message(format!("数据库初始化失败：\n\n{e}"))
                    .title("启动失败")
                    .kind(MessageDialogKind::Error)
                    .blocking_show();
                #[cfg(mobile)]
                tracing::error!(error = %e, "数据库初始化失败，带错误退出");
                std::process::exit(1);
            })?;
            let locked = app.state::<EncryptionGate>().is_locked();
            let boot_failed = app.state::<BootFailureGate>().is_failed();
            // AI 导入 HTTP 服务仅桌面启动（issue #558 / ADR-0074 决策 6）：移动端无
            // 消费方，顺带消除启动期端口绑定失败即崩的隐患；模块本体仍参与全平台
            // 编译，不落在任何 #[tauri::command] 上，命令注册扫描器零改动（ADR-0047）。
            // 传入 AppHandle：参考写入（HTTP 账号/分类 create/delete）成功后
            // emit `ledger:changed`，前端 useReferenceStore 据此自动重拉参考表（issue #79）。
            #[cfg(desktop)]
            api_server::start_http_server(
                app.handle().clone(),
                app.state::<db::DbState>().conn.clone(),
                EncryptionGate::clone(&app.state::<EncryptionGate>()),
                BootFailureGate::clone(&app.state::<BootFailureGate>()),
            );
            // 自动备份（issue #125/#126）：目录镜像为进程级单例 [`backup::shared_prefs`]，
            // 轮询调度线程与连接层写入口提交点检查（ADR-0032）共享同一份；
            // 退出兜底挂在下方 run 事件的 RunEvent::Exit 分支。
            // 锁定/启动失败期间不启动（issue #570 / #601 / ADR-0075 决策 5）：
            // 解锁先于一切业务读写，失败期间库不可用；调度分别由解锁命令与
            // 启动失败重置命令在恢复成功后拉起（轮询同轮承载定时追补）。
            if !locked && !boot_failed {
                // 后台服务成对拉起收在唯一编排点（issue #961）：自动备份 +
                // 多端同步触发，分平台分流收在域侧 `start_triggers` 一处
                // （ADR-0098 决策 4 / ADR-0074 决策 6 先例）。锁定/启动失败
                // 期间不启动（issue #570 / #601 / ADR-0075 决策 5）：解锁先于
                // 一切业务读写，失败期间库不可用；调度分别由解锁命令与启动
                // 失败重置命令在恢复成功后经同一编排点拉起。
                start_background_services(app.handle());
            }
            // 备份产物变更信号（issue #129）：自动备份的深路径执行点
            // （连接层写入口提交点的写时顺带检查）拿不到 AppHandle，启动时注入镜像句柄一次，
            // 之后经 [`events::emit_backups_changed_current`] 发射。
            events::init_event_app(app.handle());
            Ok(())
        })
        .invoke_handler(logged_invoke_handler(
            ipc_gate,
            ipc_boot_gate,
            tauri_commands_handler(),
        ))
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // 应用退出兜底（issue #125/#386）：退出前若脏且当天尚未自动备份过则补一次（日界门约束）。
            if let tauri::RunEvent::Exit = event {
                backup::exit_fallback(app);
            }
        });
}

fn logged_invoke_handler(
    gate: EncryptionGate,
    boot_gate: BootFailureGate,
    handler: impl Fn(Invoke<tauri::Wry>) -> bool + Send + Sync + 'static,
) -> impl Fn(Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    move |invoke: Invoke<tauri::Wry>| {
        let cmd = invoke.message.command().to_string();
        // 启动失败门禁（issue #601 / ADR-0075 决策 5 修订）：失败期间仅放行
        // 失败恢复屏所需的最小命令面，其余调用拒绝——占位连接不是业务库，
        // 任何业务读写都不得触达。经 resolver 回码化错误后返回 true。
        if boot_gate.is_failed() && !BOOT_FAILURE_ALLOWED_COMMANDS.contains(&cmd.as_str()) {
            tracing::warn!(command = %cmd, "启动失败期间拒绝 IPC 调用");
            invoke.resolver.reject(db::boot::gate_rejection_error());
            return true;
        }
        // 锁定门禁（issue #570 / ADR-0075 决策 5）：锁定期间仅放行解锁屏
        // 所需的最小命令面，其余调用拒绝——解锁先于一切业务读写。经
        // resolver 回码化错误后返回 true（本调用已应答，不进入命令处理）。
        if gate.is_locked() && !LOCKED_ALLOWED_COMMANDS.contains(&cmd.as_str()) {
            tracing::warn!(command = %cmd, "应用锁定期间拒绝 IPC 调用");
            invoke.resolver.reject(crate::error::AppError::coded(
                "encryption.locked",
                "应用已锁定，请先解锁后再操作",
            ));
            return true;
        }
        let payload = invoke.message.payload();
        let id_hint = match payload {
            tauri::ipc::InvokeBody::Json(value) => {
                let id_hint = extract_resource_id(value);
                tracing::info!(command = %cmd, %id_hint, "IPC 调用");
                // 参数日志经脱敏后输出：主口令字段永不落日志/trace（ADR-0075）。
                tracing::debug!(command = %cmd, payload = %redact_passphrase_payload(value), "IPC 参数");
                id_hint
            }
            _ => {
                tracing::info!(command = %cmd, "IPC 调用");
                String::new()
            }
        };
        // 归因核心：用命令 span 包裹命令执行，使数据库耗时 hook 发射的 SQL 事件
        // 自动继承调用方 span（IPC 命令均为同步函数，与 wrapper 同线程执行，归因成立；
        // 若未来引入异步命令导致丢归因，需对热点函数手包 span 兜底）。
        let span = tracing::info_span!("command", command = %cmd, %id_hint);
        let _entered = span.enter();
        handler(invoke)
    }
}

fn extract_resource_id(payload: &serde_json::Value) -> String {
    if let serde_json::Value::Object(map) = payload {
        for key in ["id", "account_id", "category_id", "occurrence_id"] {
            if let Some(val) = map.get(key) {
                if let Some(s) = val.as_str() {
                    if s.len() > 8 {
                        return format!("{}…", &s[..8]);
                    }
                    return s.to_string();
                }
                if let Some(n) = val.as_i64() {
                    return n.to_string();
                }
            }
        }
        if let Some(input) = map.get("input") {
            return extract_resource_id(input);
        }
    }
    String::new()
}
