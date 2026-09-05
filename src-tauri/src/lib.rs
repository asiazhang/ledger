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
pub mod db;
pub mod error;
pub mod events;
pub mod investment;
pub mod item;
pub mod logger;
pub mod merchants;
pub mod physical_asset;
pub mod policy;
pub mod reports;
pub mod scheduled_transactions;
pub mod settings;
pub mod signals;
pub mod sync;
// 信号守门测试（signals_cross_check，ADR-0044 决策 3 修订 / ADR-0073 决策 5）：
// 写路径接线源码扫描核对，仅测试可见。
#[cfg(test)]
mod signals_cross_check;
#[doc(hidden)]
pub mod test_utils;
pub mod transaction;
pub mod write_entry;

use tauri::Manager;
use tauri::ipc::Invoke;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

use crate::db::boot::{BOOT_DB_UNREADABLE, BootDisposition, BootFailureGate, classify_for_boot};
use crate::db::encryption::EncryptionGate;

pub mod fs_util;

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

/// IPC 日志脱敏：载荷中含主口令字段时遮蔽其值（ADR-0075 后果条款：审计日志
/// 与 trace 输出不落主口令）。按字段名匹配，对后续关闭加密/修改主口令等
/// 命令同样生效。
fn redact_passphrase_payload(payload: &serde_json::Value) -> serde_json::Value {
    // 主口令字段永不落日志/trace（ADR-0075）：解锁/开启加密的 `passphrase`
    // 与修改主口令的 `new_passphrase` 同等敏感（Tauri v2 参数名按 JS 侧
    // camelCase 到达，两种拼法都遮蔽）。
    const SENSITIVE_KEYS: &[&str] = &["passphrase", "new_passphrase", "newPassphrase"];
    match payload {
        serde_json::Value::Object(map) => {
            let mut masked = map.clone();
            for (key, value) in masked.iter_mut() {
                if SENSITIVE_KEYS.contains(&key.as_str()) && !value.is_null() {
                    *value = serde_json::Value::String("••••••".into());
                }
            }
            serde_json::Value::Object(masked)
        }
        other => other.clone(),
    }
}

fn init_database(
    app: &tauri::App,
    gate: &EncryptionGate,
    boot_gate: &BootFailureGate,
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取数据目录失败：{e}"))?;
    std::fs::create_dir_all(&dir)?;
    // 两扇进程级门先登记（后续任何路径都能取到；实例由 run() 创建并供
    // IPC/HTTP 门禁共享）：加密锁定门 + 启动失败门（issue #601）。
    app.manage(gate.clone());
    app.manage(boot_gate.clone());
    // DataLocation 引导（ADR-0018）：建连前先解析库所在目录，必要时启动期搬迁。
    let boot = db::data_location::boot(&dir);
    if let Some(reason) = &boot.fallback_reason {
        tracing::warn!(reason = %reason, "DataLocation 引导发生回退，已改用默认数据目录");
    }
    let db_dir = boot.db_dir.clone();
    // 先登记引导结果再建连：启动失败重置兜底需要知道生效目录。
    app.manage(boot);
    // 启动处置接管（issue #570 / #601 / ADR-0075 决策 4/5 修订）：真密文库
    // 不建连，进入锁定等待，解锁成功后原位换连；明文库/空文件走既有路径，
    // 零改动；头部非明文魔数且无密文库形态的损坏残留不再误入解锁等待
    // （旧缺陷：任意非魔数文件被当作密文库卡在解锁屏），按启动失败处理。
    match classify_for_boot(&db_dir.join(db::data_location::DB_FILE_NAME))? {
        BootDisposition::AwaitUnlock => {
            // 占位连接只维持 DbState 形状（IPC/HTTP 壳在锁定期间被门禁拦截，
            // 不会触达）；解锁成功后原位换成凭主口令打开的真实连接。
            manage_placeholder_db(app)?;
            gate.set_locked(true);
            tracing::info!(db_dir = %db_dir.display(), "检测到密文库，等待解锁");
        }
        BootDisposition::OpenPlaintext => {
            let db_state = db::open_db_in(&db_dir)?;
            // 日志等级接管（spec #608 / #611）：数据库就绪后按持久化档位 reload 一次
            //（此前短暂按「RUST_LOG 环境变量或默认 info」运行，属 ADR-0006 接受的启动窗口）；
            // 显式 RUST_LOG 在本次启动内优先级最高，此时不覆盖。密文库为占位连接，
            // 此步随解锁换连后（`resume_business_surface`）再做。
            {
                let conn = db_state
                    .conn
                    .lock()
                    .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
                logger::apply_persisted_level(&conn);
            }
            app.manage(db_state);
            tracing::info!("数据库初始化完成");
        }
        BootDisposition::Unreadable => {
            // 明文损坏主场景（issue #601）：库文件不可读，按启动失败处理——
            // 由 try_init_database 登记失败门、交由前端失败恢复屏接管。
            return Err(Box::new(crate::error::AppError::coded(
                BOOT_DB_UNREADABLE,
                "数据库文件无法打开（文件可能已损坏）",
            )));
        }
    }
    Ok(())
}

/// 登记占位内存连接，维持 `DbState` 形状（锁定/启动失败期间门禁拦截业务
/// IPC，占位连接不被触达；恢复通道成功后原位换入真实连接）。
fn manage_placeholder_db(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let placeholder = db::open_in_memory()?;
    app.manage(db::DbState {
        conn: std::sync::Arc::new(std::sync::Mutex::new(placeholder)),
    });
    Ok(())
}

fn try_init_database(
    app: &tauri::App,
    gate: &EncryptionGate,
    boot_gate: &BootFailureGate,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("开始初始化数据库");
    match init_database(app, gate, boot_gate) {
        Ok(()) => Ok(()),
        Err(e) => {
            // 启动失败前端接管（issue #601 / ADR-0075 决策 5 修订）：不再弹原生
            // 「重置/退出」对话框、不再退出——失败状态经 BootFailureGate 暴露给
            // 前端，由启动失败恢复屏承担恢复通道（首版通道：重置为空库，见
            // `commands::boot::reset_after_startup_failure`）。
            tracing::error!(error = %e, "数据库初始化失败，登记启动失败状态，交由前端失败恢复屏接管");
            boot_gate.set_failed();
            // 占位连接只维持 DbState 形状（IPC/HTTP 壳在失败期间被门禁拦截，
            // 不会触达）；失败恢复通道（重置）成功后原位换入真实连接。
            manage_placeholder_db(app)?;
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
    // B 类豁免（ADR-0060）：启动装配失败即无法运行——Tauri 构建失败 fail loud 退出进程。
    #[allow(clippy::expect_used)]
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .setup(move |app| {
            logger::init(app.handle());
            try_init_database(app, &gate, &boot_gate).map_err(|e| {
                // 二次失败兜底（登记失败状态也失败，如连占位内存库都打不开）：
                // 进程无法运行，fail loud 退出（B 类豁免，ADR-0060）。
                app.dialog()
                    .message(format!("数据库初始化失败：\n\n{e}"))
                    .title("启动失败")
                    .kind(MessageDialogKind::Error)
                    .blocking_show();
                std::process::exit(1);
            })?;
            let locked = app.state::<EncryptionGate>().is_locked();
            let boot_failed = app.state::<BootFailureGate>().is_failed();
            // 传入 AppHandle：参考写入（HTTP 账号/分类 create/delete）成功后
            // emit `ledger:changed`，前端 useReferenceStore 据此自动重拉参考表（issue #79）。
            api_server::start_http_server(
                app.handle().clone(),
                app.state::<db::DbState>().conn.clone(),
                EncryptionGate::clone(&app.state::<EncryptionGate>()),
                BootFailureGate::clone(&app.state::<BootFailureGate>()),
            );
            // 全量同步中断状态（issue #104）：跨命令共享运行/取消标志。
            app.manage(sync::SyncState::default());
            // 自动备份（issue #125/#126）：目录镜像为进程级单例 [`backup::shared_prefs`]，
            // 轮询调度线程与连接层写入口提交点检查（ADR-0032）共享同一份；
            // 退出兜底挂在下方 run 事件的 RunEvent::Exit 分支。
            // 锁定/启动失败期间不启动（issue #570 / #601 / ADR-0075 决策 5）：
            // 解锁先于一切业务读写，失败期间库不可用；调度分别由解锁命令与
            // 启动失败重置命令在恢复成功后拉起（轮询同轮承载定时追补）。
            if !locked && !boot_failed {
                backup::start_scheduler(app.handle());
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
