//! 设置域命令壳（spec #611）：日志等级读写（About 页「关于」Tab 入口）。
//!
//! 只做参数解包、校验、持久化与运行期接管；领域缝（闭集校验、持久化表示、滤镜接管）
//! 在 [`crate::logger`]（`LogLevel` / `set_persisted_level` / `persisted_level`）与
//! [`crate::settings`]（`SettingKey::LogLevel`）。本文件不含业务语义。
//!
//! `set_log_level` 写 `app_settings` 经 settings 模块单点收口：按 ADR-0032 置脏豁免、
//! 不发参考数据信号（设置不是账本数据），成功后才经 [`crate::logger::set_level`] 接管
//! 运行期滤镜。写操作身份 `SetLogLevel` 以例外白名单登记（见 `signals_cross_check`）。

use serde::Serialize;
use tauri::Manager;

use crate::db::{DbState, run_db};
use crate::error::{AppError, Result};
use crate::logger;

/// 日志等级当前持久化档位（设置页「关于」Tab 下拉回显）。
///
/// 只反映**持久化档位**；显式 `RUST_LOG` 环境变量在本次启动内优先且不写库，
/// 界面展示值与实际生效档位可能不一致（由「关于」页静态提示说明）。
#[derive(Debug, Serialize)]
pub struct LogLevelState {
    /// 当前持久化档位（闭集五档指令字符串之一：error / warn / info / debug / trace）。
    pub level: String,
}

/// 读取持久化日志档位（spec #611）：缺 key / 缺 `app_settings` 表（旧版本备份）回
/// 默认 info；库内残留闭集外字符串时回默认 info 并告警（读路径不因坏值上抛）。
#[tauri::command]
pub async fn get_log_level(app: tauri::AppHandle) -> Result<LogLevelState> {
    let conn = app.state::<DbState>().conn.clone();
    run_db("get_log_level", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        let level = logger::persisted_level(&conn);
        Ok(LogLevelState {
            level: level.directive().to_string(),
        })
    })
    .await
}

/// 设置日志档位（spec #611）：校验闭集（错误码 `settings.log-level-invalid`，
/// 未落库、未接管）→ 持久化到 `app_settings` → 运行期接管滤镜。改动立即生效、
/// 跨启动保留；文件与终端两条输出共用同一滤镜、一起变化。
#[tauri::command]
pub async fn set_log_level(app: tauri::AppHandle, level: String) -> Result<()> {
    let conn = app.state::<DbState>().conn.clone();
    run_db("set_log_level", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        logger::set_persisted_level(&conn, &level).map(|_| ())
    })
    .await
}
