//! 映射知识层断言（ADR-0044 决策 3）：逐写操作身份直测 [`signals_for`] 的返回值
//! 形状，含零信号显式断言——「不发」是决策行，必须可查。壳侧接线维度由根包
//! `signals_cross_check` 守卫，机制层「发射不阻塞写路径」由 `tests::emit_blocking`
//! 守卫，三者各守一个维度、并存不覆盖。

use crate::signals::*;

/// 断言信号集恰为期望值（含零信号显式断言：期望空切片）。
fn assert_signals(actual: &[Signal], expected: &[Signal]) {
    assert_eq!(actual, expected);
}

use WriteEvidence as E;
use WriteOp as Op;

// ── 参考数据四表（ADR-0012）：一律 ledger:changed ──

#[test]
fn create_account_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::CreateAccount, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn update_account_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::UpdateAccount, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn delete_account_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::DeleteAccount, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn create_category_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::CreateCategory, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn update_category_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::UpdateCategory, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn reorder_categories_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::ReorderCategories, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn delete_category_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::DeleteCategory, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn create_merchant_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::CreateMerchant, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn update_merchant_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::UpdateMerchant, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn delete_merchant_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::DeleteMerchant, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn create_insurer_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::CreateInsurer, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn update_insurer_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::UpdateInsurer, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn delete_insurer_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::DeleteInsurer, E::None),
        &[Signal::LedgerChanged],
    );
}

// ── 物品域（ADR-0014 复用 ledger:changed）──

#[test]
fn create_item_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::CreateItem, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn update_item_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::UpdateItem, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn dispose_item_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::DisposeItem, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn delete_item_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::DeleteItem, E::None),
        &[Signal::LedgerChanged],
    );
}

// ── 保单域（ADR-0051）：独立领域一律 ledger:changed ──

#[test]
fn create_policy_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::CreatePolicy, E::None),
        &[Signal::LedgerChanged],
    );
}

// ── 实物资产域（ADR-0064）：独立领域一律 ledger:changed ──

#[test]
fn create_physical_asset_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::CreatePhysicalAsset, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn update_physical_asset_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::UpdatePhysicalAsset, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn update_physical_asset_valuation_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::UpdatePhysicalAssetValuation, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn dispose_physical_asset_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::DisposePhysicalAsset, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn delete_physical_asset_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::DeletePhysicalAsset, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn update_policy_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::UpdatePolicy, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn delete_policy_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::DeletePolicy, E::None),
        &[Signal::LedgerChanged],
    );
}

// ── 余额调整：仅黑洞即建发参考信号 ──

#[test]
fn adjust_balance_emits_ledger_changed_only_when_black_hole_created() {
    // 按需新建黑洞账户 = 参考表变更 → 发。
    assert_signals(
        signals_for(Op::AdjustAccountBalance, E::BlackHoleCreated(true)),
        &[Signal::LedgerChanged],
    );
    // 纯转账（黑洞已存在）零信号。
    assert_signals(
        signals_for(Op::AdjustAccountBalance, E::BlackHoleCreated(false)),
        &[],
    );
}

// ── 价格域：共享一份「实际写入」判定 ──

#[test]
fn sync_instrument_info_emits_prices_changed_only_when_written() {
    assert_signals(
        signals_for(Op::SyncInstrumentInfo, E::PriceWritten(true)),
        &[Signal::PricesChanged],
    );
    // 零变化不广播（空库 / 全部跳过 / 基金全部「已是最新」且名称无变化）。
    assert_signals(
        signals_for(Op::SyncInstrumentInfo, E::PriceWritten(false)),
        &[],
    );
}

#[test]
fn add_fund_by_code_emits_prices_changed_only_when_price_written() {
    // 落现价即广播；未取到净值仅建标的、不广播（ADR-0038）。
    assert_signals(
        signals_for(Op::AddFundByCode, E::PriceWritten(true)),
        &[Signal::PricesChanged],
    );
    assert_signals(signals_for(Op::AddFundByCode, E::PriceWritten(false)), &[]);
}

#[test]
fn add_instrument_by_code_emits_prices_changed_only_when_price_written() {
    // 添加投资标的·场内通道（issue #697）：落现价即广播；停牌未取到价仅建
    // 标的、不广播（零变化不广播，与基金即拉同款）。
    assert_signals(
        signals_for(Op::AddInstrumentByCode, E::PriceWritten(true)),
        &[Signal::PricesChanged],
    );
    assert_signals(
        signals_for(Op::AddInstrumentByCode, E::PriceWritten(false)),
        &[],
    );
    assert_signals(signals_for(Op::AddInstrumentByCode, E::None), &[]);
}

#[test]
fn record_manual_price_emits_prices_changed_only_when_any_point_written() {
    // 实际写入任一落点（现价缓存 / 价格历史）即广播（ADR-0036）。
    assert_signals(
        signals_for(Op::RecordManualPrice, E::PriceWritten(true)),
        &[Signal::PricesChanged],
    );
    // 回填早于最新价格点且现价未动：零变化不广播。
    assert_signals(
        signals_for(Op::RecordManualPrice, E::PriceWritten(false)),
        &[],
    );
}

#[test]
fn create_instrument_emits_prices_changed_only_when_nav_persisted() {
    // HTTP 基金增强分支落现价 → 发（ADR-0039）。
    assert_signals(
        signals_for(Op::CreateInstrument, E::PriceWritten(true)),
        &[Signal::PricesChanged],
    );
    // 通用创建 / 降级创建 / IPC 手动创建：标的字典写入本身零信号。
    assert_signals(
        signals_for(Op::CreateInstrument, E::PriceWritten(false)),
        &[],
    );
    assert_signals(signals_for(Op::CreateInstrument, E::None), &[]);
}

// ── 刻意零信号：显式登记（附动机见变体文档）──

#[test]
fn create_market_price_is_deliberately_silent() {
    // 半成品写价通道（ADR-0044 决策 6）：即便误携价格证据也不发。
    assert_signals(signals_for(Op::CreateMarketPrice, E::None), &[]);
    assert_signals(
        signals_for(Op::CreateMarketPrice, E::PriceWritten(true)),
        &[],
    );
}

#[test]
fn create_exchange_rate_is_deliberately_silent() {
    // 当期汇率表不在 ledger:prices-changed 定义覆盖内（ADR-0031）。
    assert_signals(signals_for(Op::CreateExchangeRate, E::None), &[]);
}

#[test]
fn delete_instrument_is_deliberately_silent() {
    // 无流水引用的标的无消费方，前端本地重拉（issue #292）。
    assert_signals(signals_for(Op::DeleteInstrument, E::None), &[]);
}

// ── 备份域 ──

#[test]
fn create_backup_is_deliberately_silent() {
    // 前端备份组合命令成功后自刷新；收编不改现状。
    assert_signals(signals_for(Op::CreateBackup, E::None), &[]);
}

#[test]
fn prune_backups_emits_backups_changed() {
    assert_signals(
        signals_for(Op::PruneBackups, E::None),
        &[Signal::BackupsChanged],
    );
}

#[test]
fn restore_backup_is_deliberately_silent() {
    // 恢复成功后整体重启，失效信号无消费窗口。
    assert_signals(signals_for(Op::RestoreBackup, E::None), &[]);
}

#[test]
fn auto_backup_deep_path_is_registered_as_special_entry() {
    // 特例条目（ADR-0044 决策 5）：登记使生产者清单单点可查；
    // 发射走 events::emit_backups_changed_current，不经壳层 signals_for。
    assert_signals(
        signals_for(Op::AutoBackupDeepPath, E::None),
        &[Signal::BackupsChanged],
    );
}

// ── 交易域：基线零信号 + 商户即建例外 ──

#[test]
fn create_transaction_emits_ledger_changed_only_when_merchant_created() {
    // 即建商户（写第四张参考表）→ 发（修复 HTTP 导入漏发，#331 接线）。
    assert_signals(
        signals_for(Op::CreateTransaction, E::MerchantCreated(true)),
        &[Signal::LedgerChanged],
    );
    // 仅命中复用（名字命中或带 merchant_id）：零信号（不播无谓重拉）。
    assert_signals(
        signals_for(Op::CreateTransaction, E::MerchantCreated(false)),
        &[],
    );
    assert_signals(signals_for(Op::CreateTransaction, E::None), &[]);
}

#[test]
fn batch_create_transactions_emits_ledger_changed_only_when_any_merchant_created() {
    // 批内聚合「任一行即建」→ 发。
    assert_signals(
        signals_for(Op::BatchCreateTransactions, E::MerchantCreated(true)),
        &[Signal::LedgerChanged],
    );
    assert_signals(
        signals_for(Op::BatchCreateTransactions, E::MerchantCreated(false)),
        &[],
    );
}

#[test]
fn update_transaction_emits_ledger_changed_only_when_merchant_created() {
    assert_signals(
        signals_for(Op::UpdateTransaction, E::MerchantCreated(true)),
        &[Signal::LedgerChanged],
    );
    assert_signals(
        signals_for(Op::UpdateTransaction, E::MerchantCreated(false)),
        &[],
    );
}

#[test]
fn delete_transaction_is_silent() {
    assert_signals(signals_for(Op::DeleteTransaction, E::None), &[]);
}

#[test]
fn execute_scheduled_occurrence_is_silent() {
    assert_signals(signals_for(Op::ExecuteScheduledOccurrence, E::None), &[]);
}

#[test]
fn expand_scheduled_occurrences_is_silent() {
    assert_signals(signals_for(Op::ExpandScheduledOccurrences, E::None), &[]);
}

// ── 预算域：刻意零信号 ──

#[test]
fn create_budget_is_silent() {
    assert_signals(signals_for(Op::CreateBudget, E::None), &[]);
}

#[test]
fn update_budget_is_silent() {
    assert_signals(signals_for(Op::UpdateBudget, E::None), &[]);
}

#[test]
fn delete_budget_is_silent() {
    assert_signals(signals_for(Op::DeleteBudget, E::None), &[]);
}

// ── 定时计划域：刻意零信号 ──

#[test]
fn create_scheduled_transaction_is_silent() {
    assert_signals(signals_for(Op::CreateScheduledTransaction, E::None), &[]);
}

#[test]
fn update_scheduled_transaction_status_is_silent() {
    assert_signals(
        signals_for(Op::UpdateScheduledTransactionStatus, E::None),
        &[],
    );
}

#[test]
fn update_scheduled_subscription_is_silent() {
    assert_signals(signals_for(Op::UpdateScheduledSubscription, E::None), &[]);
}

// ── 设置域：刻意零信号 ──

#[test]
fn set_auto_backup_enabled_is_silent() {
    assert_signals(signals_for(Op::SetAutoBackupEnabled, E::None), &[]);
}

#[test]
fn set_auto_backup_dir_is_silent() {
    // 目录镜像推送本身零信号；首次兜底备份经 AutoBackupDeepPath 发射。
    assert_signals(signals_for(Op::SetAutoBackupDir, E::None), &[]);
}

#[test]
fn set_auto_execution_enabled_is_silent() {
    assert_signals(signals_for(Op::SetAutoExecutionEnabled, E::None), &[]);
}

#[test]
fn submit_data_location_change_is_silent() {
    // 引导指针文件写入：重启后生效，失效信号无消费窗口。
    assert_signals(signals_for(Op::SubmitDataLocationChange, E::None), &[]);
}

#[test]
fn restore_default_data_location_is_silent() {
    assert_signals(signals_for(Op::RestoreDefaultDataLocation, E::None), &[]);
}

#[test]
fn set_log_level_is_silent() {
    // 设置域（ADR-0006 / #611）：写 app_settings 的 logging.level，零信号——
    // 设置不是账本数据（ADR-0032 置脏豁免），也不属参考/价格/备份失效语义。
    assert_signals(signals_for(Op::SetLogLevel, E::None), &[]);
}

#[test]
fn set_sync_channel_config_is_silent() {
    // 多端同步通道配置（issue #862）：WebDAV 凭据写 app_settings，属本机
    // 设备配置（不同步、无失效语义），刻意零信号。
    assert_signals(signals_for(Op::SetSyncChannelConfig, E::None), &[]);
}

#[test]
fn sync_round_emits_ledger_changed_only_when_applied() {
    // 多端同步轮次（issue #862）：实际应用外来 op（applied > 0）→ 参考失效；
    // 零应用轮次（全跳过/去重/压制/挂起）→ 零变化不广播。
    assert_signals(
        signals_for(Op::SyncRound, E::LedgerApplied(true)),
        &[Signal::LedgerChanged],
    );
    assert_signals(signals_for(Op::SyncRound, E::LedgerApplied(false)), &[]);
    assert_signals(signals_for(Op::SyncRound, E::None), &[]);
}

// ── 证据形状 ──

#[test]
fn mismatched_or_missing_evidence_degrades_to_zero_signal() {
    // 证据错配保守降级（经公共接缝断言，不发错信号）：
    // 条件行拿到非本域证据或无证据，一律零信号。
    let price_ops = [
        Op::SyncInstrumentInfo,
        Op::AddFundByCode,
        Op::AddInstrumentByCode,
        Op::RecordManualPrice,
        Op::CreateInstrument,
    ];
    for op in price_ops {
        assert_signals(signals_for(op, E::None), &[]);
        assert_signals(signals_for(op, E::MerchantCreated(true)), &[]);
        assert_signals(signals_for(op, E::BlackHoleCreated(true)), &[]);
    }
    assert_signals(
        signals_for(Op::AdjustAccountBalance, E::PriceWritten(true)),
        &[],
    );
    assert_signals(
        signals_for(Op::CreateTransaction, E::PriceWritten(true)),
        &[],
    );
}
