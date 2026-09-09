//! 设置域命令壳（spec #611 / issue #858）：日志等级与本位币基准读写。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]
//!
//! 只做参数解包、校验、持久化与运行期接管；领域缝（闭集校验、持久化表示、滤镜接管、
//! 本位币基准校验与同步 op 产出）在 [`crate::logger`] 与 [`crate::currencies`]。
//! 本文件不含业务语义。
//!
//! - `set_log_level` 写 `app_settings` 经 settings 模块单点收口：按 ADR-0032 置脏豁免、
//!   不发参考数据信号（设置不是账本数据），成功后才经 [`crate::logger::set_level`] 接管
//!   运行期滤镜。写操作身份 `SetLogLevel` 以例外白名单登记（见 `signals_cross_check`）。
//! - `set_base_currency`（issue #858，LedgerLevelSetting 首个成员）必须与同步 op
//!   同事务落库（#855 纪律：写失败不残留 op），故经统一写入口 [`crate::write_entry::write_entry`]
//!   而非置脏豁免路径——本位币基准是账本级数据（随同步/备份走），置脏语义成立；
//!   信号刻意零（字典与流水未变，设置页自读回显）。

use serde::Serialize;
use tauri::Manager;

use crate::currencies::current_base_currency;
use crate::db::{DbState, run_db};
use crate::error::{AppError, Result};
use crate::logger;
use crate::signals::WriteOp;
use crate::write_entry::{Outcome, write_entry};

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
        logger::set_persisted_level(&conn, &level)
    })
    .await
}

/// 本位币基准当前值（设置页「通用」Tab 回显，issue #858）。
#[derive(Debug, Serialize)]
pub struct BaseCurrencyState {
    /// 当前基准币种代码（缺 key 回默认 CNY，随多端同步收敛全设备一致）。
    pub code: String,
}

/// 读取本位币基准（issue #858）：缺 key / 缺 `app_settings` 表（旧版本备份）
/// 回默认 CNY（读路径不因缺 key 上抛）。
#[tauri::command]
pub async fn get_base_currency(app: tauri::AppHandle) -> Result<BaseCurrencyState> {
    let conn = app.state::<DbState>().conn.clone();
    run_db("get_base_currency", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        Ok(BaseCurrencyState {
            code: current_base_currency(&conn)?,
        })
    })
    .await
}

/// 设置本位币基准（issue #858，LedgerLevelSetting 首个成员）：域编排入口校验
/// 币种字典（错误码 `currency.base-invalid`）→ 落库 → 同事务产出同步 op
/// （随多端同步分发、全设备一致，ADR-0091 决策 3）。信号刻意零：字典与流水未变，
/// 设置页自读回显；写操作身份 `SetBaseCurrency` 经本调用点声明（写入口扫描核对）。
#[tauri::command]
pub async fn set_base_currency(app: tauri::AppHandle, code: String) -> Result<BaseCurrencyState> {
    let conn = app.state::<DbState>().conn.clone();
    write_entry(
        "set_base_currency",
        conn,
        Some(&app),
        WriteOp::SetBaseCurrency,
        move |conn| {
            crate::currencies::set_base_currency(conn, &code)?;
            Ok(Outcome::Silent(BaseCurrencyState { code }))
        },
    )
    .await
}
