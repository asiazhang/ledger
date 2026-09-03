pub mod auto_run;
pub mod engine;
pub mod models;
pub mod spend;

// 域模型逐类型再导出（ADR-0059 决策 3：域 model 禁止 glob，#424 守门收口随禁令逐类型化）。
pub use auto_run::*;
pub use engine::*;
pub use models::{
    CreateScheduledInput, ExecuteOccurrenceInput, InstallmentPlan, OccurrenceStatus,
    RecurrenceType, ScheduledKind, ScheduledStatus, ScheduledTransaction,
    ScheduledTransactionDetail, ScheduledTransactionOccurrence, ScheduledTransactionWithExt,
    ScheduledTransferPlan, SubscriptionPlan, UpdateStatusInput, UpdateSubscriptionInput,
};
pub use spend::*;

#[cfg(test)]
mod tests;
