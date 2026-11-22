macro_rules! write_op_set {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident
            ),* $(,)?
        }
    ) => {
        $(#[$enum_meta])*
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant,
            )*
        }

        impl $name {
            pub const ALL: &[$name] = &[
                $($name::$variant,)*
            ];

            #[doc(hidden)]
            pub fn from_ident(ident: &str) -> Option<$name> {
                match ident {
                    $(stringify!($variant) => Some($name::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

write_op_set! {
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WriteOp {
    CreateAccount,
    UpdateAccount,
    DeleteAccount,
    ArchiveAccount,
    UnarchiveAccount,
    CreateCategory,
    UpdateCategory,
    ReorderCategories,
    DeleteCategory,
    CreateMerchant,
    UpdateMerchant,
    DeleteMerchant,
    CreateInsurer,
    UpdateInsurer,
    DeleteInsurer,

    CreateItem,
    UpdateItem,
    DisposeItem,
    DeleteItem,

    CreatePolicy,
    UpdatePolicy,
    DeletePolicy,

    CreatePhysicalAsset,
    UpdatePhysicalAsset,
    UpdatePhysicalAssetValuation,
    DisposePhysicalAsset,
    DeletePhysicalAsset,

    CreateSavingsGoal,
    UpdateSavingsGoal,
    ArchiveSavingsGoal,
    UnarchiveSavingsGoal,
    DeleteSavingsGoal,
    AdjustAccountBalance,

    SyncInstrumentInfo,
    AddFundByCode,
    AddInstrumentByCode,
    RecordManualPrice,
    CreateInstrument,
    DeleteInstrument,
    CreateMarketPrice,
    CreateExchangeRate,
    AuditBalanceCache,

    CreateBackup,
    PruneBackups,
    RestoreBackup,
    AutoBackupDeepPath,

    CreateTransaction,
    BatchCreateTransactions,
    UpdateTransaction,
    DeleteTransaction,
    ExecuteScheduledOccurrence,
    ExpandScheduledOccurrences,

    CreateBudget,
    UpdateBudget,
    DeleteBudget,

    CreateScheduledTransaction,
    UpdateScheduledTransactionStatus,
    UpdateScheduledSubscription,

    SyncRound,

    SetAutoBackupEnabled,
    SetAutoBackupDir,
    SetAutoExecutionEnabled,
    SetAutoUpdateEnabled,
    SubmitDataLocationChange,
    RestoreDefaultDataLocation,
    SetLogLevel,
    SetBaseCurrency,
    SetSyncChannelConfig,
}
}
