//! 币种域读写接缝（#2136 设置③）：本位币基准读 / 写与手动汇率同步编排直调
//! `ledger-currencies` / `ledger-market-sync` 入口——与旧壳 getBaseCurrency /
//! setBaseCurrency / syncExchangeRates 命令同一执行协议，不经 IPC。本位币是全
//! 壳折算口径的用户入口（账本级设置，随多端同步）；币种字典行集复用
//! [`super::txn_form`] 的 `currency_rows` 只读投影不另立读接缝。

use ledger_infra::db::DbState;
use ledger_infra::error::Result;

use super::conn_lock;
use crate::shell_support::write_entry_locked;

/// 读本位币基准（域权威读单点直调，缺 key 回默认 CNY——旧壳 getBaseCurrency
/// 命令同一读口径）。读连接是成对建连的只读侧。
pub fn base_currency(db: &DbState) -> Result<String> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_currencies::current_base_currency(&conn)
}

/// 设置本位币基准（域公开写编排入口直调）：币种字典成员校验在域内单点（未知
/// 代码码化拒绝 `currency.base-invalid`），落库与 op 产出同事务。写连接是成对
/// 建连的写侧。
pub fn set_base_currency(db: &DbState, code: &str) -> Result<()> {
    write_entry_locked(db, |conn| ledger_currencies::set_base_currency(conn, code))
}

/// 手动汇率同步编排接缝（#2136 设置③；旧壳 `syncExchangeRates` IPC 命令壳的
/// 本壳等价形，消费面在 [`crate::settings_currencies`] 同步卡，不经 IPC）：
/// 门面写槽裸作业会话交给市场同步域的持门编排入口（[`ledger_market_sync::
/// sync_fx_rates_guarded`]——手动入口与每日自动车道共用进程级在途互斥门
/// [`ledger_market_sync::FX_SYNC_GATE`]，撞车即码化错误 `fx.sync-in-progress`），
/// 生产通道束每次同步构造（ECB 官方站两腿，域内单点），进度回调由壳侧注入。
/// 同步语义（深度判据 / 全量回填 / 增量 / 幂等落库 / 三态错误）全在域内单正文，
/// 本壳只触发接缝与呈现报告。
pub async fn sync_exchange_rates(
    db: &DbState,
    mut progress: impl FnMut(ledger_market_sync::FxSyncProgress) + Send,
) -> Result<ledger_market_sync::FxSyncReport> {
    let session =
        ledger_market_sync::FacadeWriteSession::new(db.write_handle(), "sync_exchange_rates");
    let mut channels = match take_injected_fx_sync_channels_for_tests() {
        Some(channels) => channels,
        None => ledger_market_sync::FxSyncChannels::production()?,
    };
    ledger_market_sync::sync_fx_rates_guarded(
        &ledger_market_sync::FX_SYNC_GATE,
        &session,
        &mut channels,
        &mut progress,
    )
    .await
}

/// 手动汇率同步的通道束注入槽（headless 测试接缝，`#[doc(hidden)]` 生产不
/// 消费：`Some` = 一次性覆写生产通道束，接线取走后回空）。测试注入桩束
/// 钉「同步卡触发接线到域编排」的半边（删 boot 接线或卡内触发即红）。
static TEST_FX_CHANNELS: std::sync::Mutex<Option<ledger_market_sync::FxSyncChannels>> =
    std::sync::Mutex::new(None);

/// 注入手动汇率同步桩通道束（headless 测试消费）。
#[doc(hidden)]
pub fn inject_fx_sync_channels_for_tests(channels: ledger_market_sync::FxSyncChannels) {
    *TEST_FX_CHANNELS.lock().unwrap_or_else(|p| p.into_inner()) = Some(channels);
}

/// 取走注入的桩通道束（一次性；生产恒 None）。
#[doc(hidden)]
pub fn take_injected_fx_sync_channels_for_tests() -> Option<ledger_market_sync::FxSyncChannels> {
    TEST_FX_CHANNELS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
}
