use super::evidence::WriteEvidence;
use super::write_op::WriteOp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    LedgerChanged,
    PricesChanged,
    BackupsChanged,
}

const NO_SIGNALS: &[Signal] = &[];

const LEDGER_CHANGED_SET: &[Signal] = &[Signal::LedgerChanged];
const PRICES_CHANGED_SET: &[Signal] = &[Signal::PricesChanged];
const BACKUPS_CHANGED_SET: &[Signal] = &[Signal::BackupsChanged];

fn when(cond: bool, signals: &'static [Signal]) -> &'static [Signal] {
    if cond { signals } else { NO_SIGNALS }
}

pub fn signals_for(op: WriteOp, evidence: WriteEvidence) -> &'static [Signal] {
    match op {
        WriteOp::CreateAccount
        | WriteOp::UpdateAccount
        | WriteOp::DeleteAccount
        | WriteOp::ArchiveAccount
        | WriteOp::UnarchiveAccount
        | WriteOp::CreateCategory
        | WriteOp::UpdateCategory
        | WriteOp::ReorderCategories
        | WriteOp::DeleteCategory
        | WriteOp::CreateMerchant
        | WriteOp::UpdateMerchant
        | WriteOp::DeleteMerchant
        | WriteOp::CreateInsurer
        | WriteOp::UpdateInsurer
        | WriteOp::DeleteInsurer => LEDGER_CHANGED_SET,

        WriteOp::CreateItem | WriteOp::UpdateItem | WriteOp::DisposeItem | WriteOp::DeleteItem => {
            LEDGER_CHANGED_SET
        }

        WriteOp::CreatePolicy | WriteOp::UpdatePolicy | WriteOp::DeletePolicy => LEDGER_CHANGED_SET,

        WriteOp::CreatePhysicalAsset
        | WriteOp::UpdatePhysicalAsset
        | WriteOp::UpdatePhysicalAssetValuation
        | WriteOp::DisposePhysicalAsset
        | WriteOp::DeletePhysicalAsset => LEDGER_CHANGED_SET,

        WriteOp::CreateSavingsGoal
        | WriteOp::UpdateSavingsGoal
        | WriteOp::ArchiveSavingsGoal
        | WriteOp::UnarchiveSavingsGoal
        | WriteOp::DeleteSavingsGoal => LEDGER_CHANGED_SET,

        WriteOp::AdjustAccountBalance => when(evidence.black_hole_created(), LEDGER_CHANGED_SET),
        WriteOp::AuditBalanceCache => NO_SIGNALS,

        WriteOp::SyncInstrumentInfo
        | WriteOp::AddFundByCode
        | WriteOp::AddInstrumentByCode
        | WriteOp::RecordManualPrice
        | WriteOp::CreateInstrument => when(evidence.price_written(), PRICES_CHANGED_SET),

        WriteOp::CreateMarketPrice => NO_SIGNALS,
        WriteOp::CreateExchangeRate => NO_SIGNALS,
        WriteOp::DeleteInstrument => NO_SIGNALS,

        WriteOp::CreateBackup => NO_SIGNALS,
        WriteOp::PruneBackups => BACKUPS_CHANGED_SET,
        WriteOp::RestoreBackup => NO_SIGNALS,
        WriteOp::AutoBackupDeepPath => BACKUPS_CHANGED_SET,

        WriteOp::CreateTransaction
        | WriteOp::BatchCreateTransactions
        | WriteOp::UpdateTransaction => when(evidence.merchant_created(), LEDGER_CHANGED_SET),
        WriteOp::DeleteTransaction
        | WriteOp::ExecuteScheduledOccurrence
        | WriteOp::ExpandScheduledOccurrences => NO_SIGNALS,

        WriteOp::CreateBudget | WriteOp::UpdateBudget | WriteOp::DeleteBudget => NO_SIGNALS,

        WriteOp::CreateScheduledTransaction
        | WriteOp::UpdateScheduledTransactionStatus
        | WriteOp::UpdateScheduledSubscription => NO_SIGNALS,

        WriteOp::SetAutoBackupEnabled
        | WriteOp::SetAutoBackupDir
        | WriteOp::SetAutoExecutionEnabled
        | WriteOp::SetAutoUpdateEnabled
        | WriteOp::SubmitDataLocationChange
        | WriteOp::RestoreDefaultDataLocation
        | WriteOp::SetLogLevel
        | WriteOp::SetBaseCurrency
        | WriteOp::SetSyncChannelConfig => NO_SIGNALS,

        WriteOp::SyncRound => when(evidence.ledger_applied(), LEDGER_CHANGED_SET),
    }
}
