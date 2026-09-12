pub mod auto_run;
pub mod command;
pub mod engine;
pub mod models;
pub mod spend;
// 计划来源反查（spec #704 / #1090 接缝反转实现侧）：模块私有——反查行不公开
// 再导出，壳层接线只经本入口的 `install_plan_source_hook`。
mod source;

// 域模型逐类型再导出（ADR-0059 决策 3：域 model 禁止 glob，#424 守门收口随禁令逐类型化）。
pub use auto_run::*;
pub(crate) use command::replay_command;
pub use command::{ScheduledCommand, occurrence_transaction_id};
pub use engine::*;
pub use models::{
    CreateScheduledInput, ExecuteOccurrenceInput, InstallmentPlan, OccurrenceStatus,
    RecurrenceType, ScheduledKind, ScheduledStatus, ScheduledTransaction,
    ScheduledTransactionDetail, ScheduledTransactionOccurrence, ScheduledTransactionWithExt,
    ScheduledTransferPlan, SubscriptionPlan, UpdateStatusInput, UpdateSubscriptionInput,
};
pub use source::install_plan_source_hook;
pub use spend::*;

#[cfg(test)]
mod tests;
