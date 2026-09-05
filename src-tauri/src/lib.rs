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
// 信号交叉核对测试（ADR-0044 决策 3 / #335）：声明表 × 映射表双向核对，仅测试可见。
#[cfg(test)]
mod signals_cross_check;
#[doc(hidden)]
pub mod test_utils;
pub mod transaction;

use tauri::Manager;
use tauri::ipc::Invoke;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::db::encryption::{DbFileKind, EncryptionGate, probe_file_kind};

pub mod fs_util;

// 命令注册单一来源（ADR-0047）：由 build.rs 扫描 #[tauri::command] 注解生成、
// include! 进本 crate；命令注册零手工清单，新增/删除命令只改命令域文件本身。
include!(concat!(env!("OUT_DIR"), "/commands_registry.rs"));

/// 锁定期间放行的 IPC 命令白名单（issue #570 / ADR-0075 决策 5）：解锁屏
/// 启动期所需的最小面（状态查询、解锁、忘记口令重置 #573）；其余命令在
/// 解锁前一律拒绝（解锁先于一切业务读写）。
const LOCKED_ALLOWED_COMMANDS: &[&str] = &[
    "get_encryption_status",
    "unlock_encryption",
    "reset_after_forgotten_passphrase",
];

/// IPC 日志脱敏：载荷中含主口令字段时遮蔽其值（ADR-0075 后果条款：审计日志
/// 与 trace 输出不落主口令）。按字段名匹配，对后续关闭加密/修改主口令等
/// 命令同样生效。
fn redact_passphrase_payload(payload: &serde_json::Value) -> serde_json::Value {
    const SENSITIVE_KEYS: &[&str] = &["passphrase"];
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
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取数据目录失败：{e}"))?;
    std::fs::create_dir_all(&dir)?;
    // 锁定门先登记（后续任何路径都能取到；实例由 run() 创建并供 IPC 门禁共享）。
    app.manage(gate.clone());
    // DataLocation 引导（ADR-0018）：建连前先解析库所在目录，必要时启动期搬迁。
    let boot = db::data_location::boot(&dir);
    if let Some(reason) = &boot.fallback_reason {
        tracing::warn!(reason = %reason, "DataLocation 引导发生回退，已改用默认数据目录");
    }
    let db_dir = boot.db_dir.clone();
    // 先登记引导结果再建连：启动失败重置兜底需要知道生效目录。
    app.manage(boot);
    // 启动探测接管（issue #570 / ADR-0075 决策 4/5）：密文库不建连，进入
    // 锁定等待，解锁成功后原位换连；明文库/空文件走既有路径，零改动。
    match probe_file_kind(&db_dir.join(db::data_location::DB_FILE_NAME))? {
        DbFileKind::Encrypted => {
            // 占位连接只维持 DbState 形状（IPC/HTTP 壳在锁定期间被门禁拦截，
            // 不会触达）；解锁成功后原位换成凭主口令打开的真实连接。
            let placeholder = db::open_in_memory()?;
            app.manage(db::DbState {
                conn: std::sync::Arc::new(std::sync::Mutex::new(placeholder)),
            });
            gate.set_locked(true);
            tracing::info!(db_dir = %db_dir.display(), "检测到密文库，等待解锁");
        }
        DbFileKind::Plaintext | DbFileKind::Empty => {
            let db_state = db::open_db_in(&db_dir)?;
            app.manage(db_state);
            tracing::info!("数据库初始化完成");
        }
    }
    Ok(())
}

fn try_init_database(
    app: &tauri::App,
    gate: &EncryptionGate,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("开始初始化数据库");
    match init_database(app, gate) {
        Ok(()) => Ok(()),
        Err(e) => {
            tracing::error!(error = %e, "数据库初始化失败");
            let confirmed = app
                .dialog()
                .message(format!(
                    "数据库初始化失败：\n\n{e}\n\n是否备份旧数据并重置数据库？"
                ))
                .title("数据库错误")
                .kind(MessageDialogKind::Warning)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    "重置数据库".into(),
                    "退出".into(),
                ))
                .blocking_show();
            if confirmed {
                // 重置兜底作用于 DataLocation 引导解析出的生效目录（引导结果
                // 在建连前已登记；引导本身失败时落到默认数据目录）。
                let db_dir = match app.try_state::<db::data_location::Boot>() {
                    Some(boot) => boot.db_dir.clone(),
                    None => {
                        let dir = app
                            .path()
                            .app_data_dir()
                            .map_err(|e| format!("获取数据目录失败：{e}"))?;
                        std::fs::create_dir_all(&dir)?;
                        dir
                    }
                };
                let db_state = db::reset_db_in(&db_dir)?;
                app.manage(db_state);
                // 重置后得到的是新建明文库（锁定门保持不锁）；gate 已在
                // init_database 开头登记，此处无需重复 manage。
                Ok(())
            } else {
                std::process::exit(0);
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 加密锁定门（issue #570）：实例在装配期创建，同一份分别供 IPC 门禁
    //（invoke wrapper 捕获）与启动编排（setup 登记）消费。
    let gate = EncryptionGate::new(false);
    let ipc_gate = gate.clone();
    // B 类豁免（ADR-0060）：启动装配失败即无法运行——Tauri 构建失败 fail loud 退出进程。
    #[allow(clippy::expect_used)]
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .setup(move |app| {
            logger::init(app.handle());
            try_init_database(app, &gate).map_err(|e| {
                app.dialog()
                    .message(format!("数据库初始化失败：\n\n{e}"))
                    .title("启动失败")
                    .kind(MessageDialogKind::Error)
                    .blocking_show();
                std::process::exit(1);
            })?;
            let locked = app.state::<EncryptionGate>().is_locked();
            // 传入 AppHandle：参考写入（HTTP 账号/分类 create/delete）成功后
            // emit `ledger:changed`，前端 useReferenceStore 据此自动重拉参考表（issue #79）。
            api_server::start_http_server(
                app.handle().clone(),
                app.state::<db::DbState>().conn.clone(),
                EncryptionGate::clone(&app.state::<EncryptionGate>()),
            );
            // 全量同步中断状态（issue #104）：跨命令共享运行/取消标志。
            app.manage(sync::SyncState::default());
            // 自动备份（issue #125/#126）：目录镜像为进程级单例 [`backup::shared_prefs`]，
            // 轮询调度线程与连接层写入口提交点检查（ADR-0032）共享同一份；
            // 退出兜底挂在下方 run 事件的 RunEvent::Exit 分支。
            // 锁定期间不启动（issue #570 / ADR-0075 决策 5）：解锁先于一切业务
            // 读写，调度由解锁命令在解锁成功后拉起（轮询同轮承载定时追补）。
            if !locked {
                backup::start_scheduler(app.handle());
            }
            // 备份产物变更信号（issue #129）：自动备份的深路径执行点
            // （连接层写入口提交点的写时顺带检查）拿不到 AppHandle，启动时注入镜像句柄一次，
            // 之后经 [`events::emit_backups_changed_current`] 发射。
            events::init_event_app(app.handle());
            Ok(())
        })
        .invoke_handler(logged_invoke_handler(ipc_gate, tauri_commands_handler()))
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
    handler: impl Fn(Invoke<tauri::Wry>) -> bool + Send + Sync + 'static,
) -> impl Fn(Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    move |invoke: Invoke<tauri::Wry>| {
        let cmd = invoke.message.command().to_string();
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
