use crate::signals::*;

fn assert_signals(actual: &[Signal], expected: &[Signal]) {
    assert_eq!(actual, expected);
}

use WriteEvidence as E;
use WriteOp as Op;

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

#[test]
fn create_policy_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::CreatePolicy, E::None),
        &[Signal::LedgerChanged],
    );
}

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
fn create_savings_goal_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::CreateSavingsGoal, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn update_savings_goal_emits_ledger_changed() {
    assert_signals(
        signals_for(Op::UpdateSavingsGoal, E::None),
        &[Signal::LedgerChanged],
    );
}

#[test]
fn archive_unarchive_delete_savings_goal_emit_ledger_changed() {
    assert_signals(
        signals_for(Op::ArchiveSavingsGoal, E::None),
        &[Signal::LedgerChanged],
    );
    assert_signals(
        signals_for(Op::UnarchiveSavingsGoal, E::None),
        &[Signal::LedgerChanged],
    );
    assert_signals(
        signals_for(Op::DeleteSavingsGoal, E::None),
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

#[test]
fn adjust_balance_emits_ledger_changed_only_when_black_hole_created() {
    assert_signals(
        signals_for(Op::AdjustAccountBalance, E::BlackHoleCreated(true)),
        &[Signal::LedgerChanged],
    );
    assert_signals(
        signals_for(Op::AdjustAccountBalance, E::BlackHoleCreated(false)),
        &[],
    );
}

#[test]
fn sync_instrument_info_emits_prices_changed_only_when_written() {
    assert_signals(
        signals_for(Op::SyncInstrumentInfo, E::PriceWritten(true)),
        &[Signal::PricesChanged],
    );
    assert_signals(
        signals_for(Op::SyncInstrumentInfo, E::PriceWritten(false)),
        &[],
    );
}

#[test]
fn add_fund_by_code_emits_prices_changed_only_when_price_written() {
    assert_signals(
        signals_for(Op::AddFundByCode, E::PriceWritten(true)),
        &[Signal::PricesChanged],
    );
    assert_signals(signals_for(Op::AddFundByCode, E::PriceWritten(false)), &[]);
}

#[test]
fn add_instrument_by_code_emits_prices_changed_only_when_price_written() {
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
    assert_signals(
        signals_for(Op::RecordManualPrice, E::PriceWritten(true)),
        &[Signal::PricesChanged],
    );
    assert_signals(
        signals_for(Op::RecordManualPrice, E::PriceWritten(false)),
        &[],
    );
}

#[test]
fn create_instrument_emits_prices_changed_only_when_nav_persisted() {
    assert_signals(
        signals_for(Op::CreateInstrument, E::PriceWritten(true)),
        &[Signal::PricesChanged],
    );
    assert_signals(
        signals_for(Op::CreateInstrument, E::PriceWritten(false)),
        &[],
    );
    assert_signals(signals_for(Op::CreateInstrument, E::None), &[]);
}

#[test]
fn create_market_price_is_deliberately_silent() {
    assert_signals(signals_for(Op::CreateMarketPrice, E::None), &[]);
    assert_signals(
        signals_for(Op::CreateMarketPrice, E::PriceWritten(true)),
        &[],
    );
}

#[test]
fn create_exchange_rate_is_deliberately_silent() {
    assert_signals(signals_for(Op::CreateExchangeRate, E::None), &[]);
}

#[test]
fn delete_instrument_is_deliberately_silent() {
    assert_signals(signals_for(Op::DeleteInstrument, E::None), &[]);
}

#[test]
fn create_backup_is_deliberately_silent() {
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
    assert_signals(signals_for(Op::RestoreBackup, E::None), &[]);
}

#[test]
fn auto_backup_deep_path_is_registered_as_special_entry() {
    assert_signals(
        signals_for(Op::AutoBackupDeepPath, E::None),
        &[Signal::BackupsChanged],
    );
}

#[test]
fn create_transaction_emits_ledger_changed_only_when_merchant_created() {
    assert_signals(
        signals_for(Op::CreateTransaction, E::MerchantCreated(true)),
        &[Signal::LedgerChanged],
    );
    assert_signals(
        signals_for(Op::CreateTransaction, E::MerchantCreated(false)),
        &[],
    );
    assert_signals(signals_for(Op::CreateTransaction, E::None), &[]);
}

#[test]
fn batch_create_transactions_emits_ledger_changed_only_when_any_merchant_created() {
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

#[test]
fn set_auto_backup_enabled_is_silent() {
    assert_signals(signals_for(Op::SetAutoBackupEnabled, E::None), &[]);
}

#[test]
fn set_auto_backup_dir_is_silent() {
    assert_signals(signals_for(Op::SetAutoBackupDir, E::None), &[]);
}

#[test]
fn set_auto_execution_enabled_is_silent() {
    assert_signals(signals_for(Op::SetAutoExecutionEnabled, E::None), &[]);
}

#[test]
fn set_auto_update_enabled_is_silent() {
    assert_signals(signals_for(Op::SetAutoUpdateEnabled, E::None), &[]);
}

#[test]
fn submit_data_location_change_is_silent() {
    assert_signals(signals_for(Op::SubmitDataLocationChange, E::None), &[]);
}

#[test]
fn restore_default_data_location_is_silent() {
    assert_signals(signals_for(Op::RestoreDefaultDataLocation, E::None), &[]);
}

#[test]
fn set_log_level_is_silent() {
    assert_signals(signals_for(Op::SetLogLevel, E::None), &[]);
}

#[test]
fn set_sync_channel_config_is_silent() {
    assert_signals(signals_for(Op::SetSyncChannelConfig, E::None), &[]);
}

#[test]
fn sync_round_emits_ledger_changed_only_when_applied() {
    assert_signals(
        signals_for(Op::SyncRound, E::LedgerApplied(true)),
        &[Signal::LedgerChanged],
    );
    assert_signals(signals_for(Op::SyncRound, E::LedgerApplied(false)), &[]);
    assert_signals(signals_for(Op::SyncRound, E::None), &[]);
}

#[test]
fn mismatched_or_missing_evidence_degrades_to_zero_signal() {
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
