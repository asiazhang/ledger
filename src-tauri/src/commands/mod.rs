//! IPC 命令壳枢纽：命令域模块声明 + 扁平 `pub use` 链（注册路径 `commands::<name>`，
//! ADR-0047）+ IPC 壳「命令 → 写操作身份」声明表（ADR-0044 / #335，见
//! [`IPC_COMMAND_WRITE_OPS`]）。

use crate::signals::WriteOp;

pub mod accounts;
pub mod ai;
pub mod backup;
pub mod budget;
pub mod categories;
pub mod currencies;
pub mod dashboard;
pub mod data_location;
pub mod financial_freedom;
pub mod investment;
pub mod item;
pub mod logs;
pub mod merchants;
pub mod physical_asset;
pub mod policy;
pub mod reports;
pub mod scheduled;
pub mod search;
pub mod sync;
pub mod transactions;

pub use accounts::*;
pub use ai::*;
pub use backup::*;
pub use budget::*;
pub use categories::*;
pub use currencies::*;
pub use dashboard::*;
pub use data_location::*;
pub use financial_freedom::*;
pub use investment::*;
pub use item::*;
pub use logs::*;
pub use merchants::*;
pub use physical_asset::*;
pub use policy::*;
pub use reports::*;
pub use scheduled::*;
pub use search::*;
pub use sync::*;
pub use transactions::*;

/// IPC 壳「命令 → 写操作身份」声明表（ADR-0044 决策 3 / #335）：本壳注册面上每个
/// `#[tauri::command]` 的逐一身份声明——写命令声明其 [`WriteOp`]（与命令体内
/// `signals::emit_for` 的判定键同源），读 / 控制命令显式 [`None`]（刻意无写身份，
/// 是决策而非遗漏）。
///
/// 交叉核对测试（`signals_cross_check`）以 build.rs 生成的 `IPC_COMMAND_MANIFEST`
/// （注册清单真源，ADR-0047 派生物）与本表双向比对：「新写命令忘了声明身份」
/// 与「表键漂移（命令改名未同步 / 手误）」测试期即红；`Some` 身份再经
/// `signals_for` 的编译期穷尽 match 保证映射不缺行。
///
/// **新增 / 删除 / 改名命令必须同步本表**——这是信号知识的壳侧接线声明，
/// 漏声明不是「不发信号」的合法形态。
pub const IPC_COMMAND_WRITE_OPS: &[(&str, Option<WriteOp>)] = &[
    // ── 账户域 ──
    ("create_account", Some(WriteOp::CreateAccount)),
    ("update_account", Some(WriteOp::UpdateAccount)),
    ("delete_account", Some(WriteOp::DeleteAccount)),
    (
        "adjust_account_balance",
        Some(WriteOp::AdjustAccountBalance),
    ),
    ("list_accounts", None),
    ("list_account_balances", None),
    // ── AI 导入 ──
    ("get_ai_prompt", None),
    // ── 备份域 ──
    ("create_backup", Some(WriteOp::CreateBackup)),
    ("restore_backup", Some(WriteOp::RestoreBackup)),
    ("prune_backups", Some(WriteOp::PruneBackups)),
    (
        "set_auto_backup_enabled",
        Some(WriteOp::SetAutoBackupEnabled),
    ),
    ("set_auto_backup_dir", Some(WriteOp::SetAutoBackupDir)),
    ("get_auto_backup_state", None),
    ("list_backups", None),
    ("restart_app", None), // 控制类：应用重启
    // ── 预算域 ──
    ("create_budget", Some(WriteOp::CreateBudget)),
    ("update_budget", Some(WriteOp::UpdateBudget)),
    ("delete_budget", Some(WriteOp::DeleteBudget)),
    ("list_budgets", None),
    ("budget_progress", None),
    // ── 分类域 ──
    ("create_category", Some(WriteOp::CreateCategory)),
    ("update_category", Some(WriteOp::UpdateCategory)),
    ("reorder_categories", Some(WriteOp::ReorderCategories)),
    ("delete_category", Some(WriteOp::DeleteCategory)),
    ("list_categories", None),
    // ── 币种（参考表，种子权威、无写命令）──
    ("list_currencies", None),
    // ── 仪表盘 ──
    ("dashboard_overview", None),
    // ── 财务自由度（只读聚合，issue #343）──
    ("financial_freedom", None),
    // ── 数据位置 ──
    (
        "submit_data_location_change",
        Some(WriteOp::SubmitDataLocationChange),
    ),
    (
        "restore_default_data_location",
        Some(WriteOp::RestoreDefaultDataLocation),
    ),
    ("get_data_location_info", None),
    // ── 投资域 ──
    ("create_instrument", Some(WriteOp::CreateInstrument)),
    ("delete_instrument", Some(WriteOp::DeleteInstrument)),
    ("create_market_price", Some(WriteOp::CreateMarketPrice)),
    ("create_exchange_rate", Some(WriteOp::CreateExchangeRate)),
    ("add_fund_by_code", Some(WriteOp::AddFundByCode)),
    ("record_manual_price", Some(WriteOp::RecordManualPrice)),
    ("list_instruments", None),
    ("list_market_prices", None),
    ("list_exchange_rates", None),
    ("list_holdings", None),
    ("instrument_price_trend", None),
    ("portfolio_value_trend", None),
    ("realized_pnl_summary", None),
    ("get_transaction_trade", None),
    // ── 物品域 ──
    ("create_item", Some(WriteOp::CreateItem)),
    ("update_item", Some(WriteOp::UpdateItem)),
    ("dispose_item", Some(WriteOp::DisposeItem)),
    ("delete_item", Some(WriteOp::DeleteItem)),
    ("list_items", None),
    ("calculate_item_cost", None),
    ("item_daily_total", None),
    // ── 日志 ──
    ("open_log_dir", None), // 控制类：打开日志目录
    // ── 保单域（静态档案，issue #360 / ADR-0051）──
    ("create_policy", Some(WriteOp::CreatePolicy)),
    ("update_policy", Some(WriteOp::UpdatePolicy)),
    ("delete_policy", Some(WriteOp::DeletePolicy)),
    ("list_policies", None),
    // 保单视角统计（issue #363，只读聚合）
    ("list_policy_stats", None),
    // ── 商户域 ──
    ("create_merchant", Some(WriteOp::CreateMerchant)),
    ("update_merchant", Some(WriteOp::UpdateMerchant)),
    ("delete_merchant", Some(WriteOp::DeleteMerchant)),
    ("list_merchants", None),
    // 商户关联交易计数（issue #445，只读聚合）
    ("list_merchant_transaction_counts", None),
    // ── 实物资产域（issue #466 T1 / #467 T2 / #468 T3 / ADR-0064）──
    ("create_physical_asset", Some(WriteOp::CreatePhysicalAsset)),
    ("update_physical_asset", Some(WriteOp::UpdatePhysicalAsset)),
    (
        "update_physical_asset_valuation",
        Some(WriteOp::UpdatePhysicalAssetValuation),
    ),
    (
        "dispose_physical_asset",
        Some(WriteOp::DisposePhysicalAsset),
    ),
    ("delete_physical_asset", Some(WriteOp::DeletePhysicalAsset)),
    ("list_physical_assets", None),
    ("get_physical_asset", None),
    // ── 报表 ──
    ("monthly_summary", None),
    ("merchant_shares", None),
    ("report_date_range", None),
    ("category_shares", None),
    // ── 定时计划域 ──
    (
        "create_scheduled_transaction",
        Some(WriteOp::CreateScheduledTransaction),
    ),
    (
        "update_scheduled_transaction_status",
        Some(WriteOp::UpdateScheduledTransactionStatus),
    ),
    (
        "update_scheduled_subscription",
        Some(WriteOp::UpdateScheduledSubscription),
    ),
    (
        "execute_scheduled_occurrence",
        Some(WriteOp::ExecuteScheduledOccurrence),
    ),
    (
        "expand_scheduled_occurrences",
        Some(WriteOp::ExpandScheduledOccurrences),
    ),
    (
        "set_auto_execution_enabled",
        Some(WriteOp::SetAutoExecutionEnabled),
    ),
    ("list_scheduled_transactions", None),
    ("get_scheduled_transaction_detail", None),
    ("subscription_spend_overview", None),
    // ── 搜索 ──
    ("search_transactions", None),
    // ── 行情同步域 ──
    ("sync_holding_prices", Some(WriteOp::SyncHoldingPrices)),
    ("sync_instruments", Some(WriteOp::SyncInstruments)),
    ("cancel_sync_instruments", None), // 控制类：协作取消全量同步
    // ── 交易域 ──
    ("create_transaction", Some(WriteOp::CreateTransaction)),
    (
        "create_transactions",
        Some(WriteOp::BatchCreateTransactions),
    ),
    ("update_transaction", Some(WriteOp::UpdateTransaction)),
    ("delete_transaction", Some(WriteOp::DeleteTransaction)),
    ("list_transactions", None),
];
