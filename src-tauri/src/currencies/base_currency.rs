//! 本位币基准（LedgerLevelSetting 首个成员，issue #858 / ADR-0091 决策 3）。
//!
//! DefaultCurrency 的后端本位币基准升格为账本级设置：随多端同步分发、全设备
//! 强制一致，是 native 口径唯一的根据（ADR-0091 决策 3）；存储落点仍遵守
//! ADR-0017 判定——后端消费的配置存 `app_settings`（键 `ledger.base_currency`），
//! 账本级只新增「跨设备真值」的分发语义，不改落点规则。前端展示币种偏好与本
//! 设置互不相干：前者是轻量设备偏好（localStorage，不随同步），见参考数据与
//! 设置域「轻量设置项」。
//!
//! 接缝：
//! - [`current`]：权威读单点（缺 key / 缺表回默认 [`DEFAULT_BASE_CURRENCY`]，
//!   与 `settings::get` 兑底语义一致）；
//! - [`set_base_currency`]：本地写编排入口（校验 → 落库 → 产出 op，同事务）；
//! - [`replay_command`]：重放执行（与本地写同协议，不产出 op——外来 op 由
//!   同步引擎落日志）。
//!
//! 折算消费方为 Amount 接缝（`transaction::amount`），经域路径显式 import。

use rusqlite::{Connection, OptionalExtension};

use crate::error::{AppError, Result};
use crate::settings;

/// 本位币基准的默认值（与种子数据一致；缺 key / 旧版本备份恢复后缺表时回此值，
/// 行为免费正确）。
pub const DEFAULT_BASE_CURRENCY: &str = "CNY";

/// 读取本位币基准（权威读单点）：`app_settings` 的 `ledger.base_currency` 键。
pub fn current_base_currency(conn: &Connection) -> Result<String> {
    settings::get(
        conn,
        settings::SettingKey::LedgerBaseCurrency,
        DEFAULT_BASE_CURRENCY.to_string(),
    )
}

/// 校验 + 落库（本地写与重放共用的执行协议，不含 op 产出）。
///
/// 币种代码必须是币种字典的既有成员（种子权威参考数据）：未知代码返回码化错误
/// `currency.base-invalid`（不落库、不产出 op），防止折算基准指向不存在的币种。
fn write_setting(conn: &Connection, code: &str) -> Result<()> {
    let known: Option<i64> = conn
        .query_row("SELECT 1 FROM currencies WHERE code = ?1", [code], |r| {
            r.get(0)
        })
        .optional()?;
    if known.is_none() {
        return Err(AppError::codedp(
            "currency.base-invalid",
            format!("本位币基准必须是币种字典中的币种：{code}"),
            &[code],
        ));
    }
    settings::set(conn, settings::SettingKey::LedgerBaseCurrency, &code)
}

/// 设置本位币基准（本地写编排入口）：校验、落库与 op 产出同事务——调用方保证
/// 处于写事务内（IPC 壳经统一写入口），任一步失败整体回滚，不残留半套状态。
pub fn set_base_currency(conn: &Connection, code: &str) -> Result<()> {
    write_setting(conn, code)?;
    super::command::record_local(
        conn,
        super::command::LedgerSettingCommand::SetBaseCurrency {
            code: code.to_string(),
        },
    )
}

/// 重放执行（同步引擎分派接缝）：与本地写同一执行协议（校验 + 落库）；不产出
/// op——外来 op 由同步引擎在重放事务内落日志，命令执行不得再追加本地 op。
pub(crate) fn replay_set_base_currency(conn: &Connection, code: &str) -> Result<()> {
    write_setting(conn, code)
}

// ---------------------------------------------------------------------------
// 交易×币种接缝实现（spec #1086 / issue #1092）
// ---------------------------------------------------------------------------

/// 注册本位币基准读取实现（核心交易域 `transaction::base_currency_seam` 注册点，
/// #1092）：把本域权威读单点 [`current_base_currency`] 装入，壳层启动接线，
/// 业务代码不直接调用。
pub fn install_base_currency_hook() {
    crate::transaction::base_currency_seam::register_base_currency_reader(current_base_currency);
}
