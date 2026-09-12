//! 交易×来源域接缝（跨域接缝区，issue #1090 / #1092）：来源列①保单 / ②计划 /
//! ③物品的反查注册点与委派单点。
//!
//! 职责：本域只定义注册点与契约投影（[`TransactionSource`] 映射），实现由保单 /
//! 定时计划 / 物品域装入、壳层启动时接线（ADR-0112 决策 5）。不变量：未注册即
//! 码化错误（读取显式失败，不静默丢来源列）。ADR 指针：ADR-0112 决策 5 /
//! ADR-0113 决策 8。陷阱：消费逻辑留读路径 `crate::read::source`，本模块不读列表。

use std::collections::HashMap;
use std::sync::OnceLock;

use rusqlite::Connection;

use ledger_infra::error::{AppError, Result};

use crate::model::TransactionSource;

// ---------------------------------------------------------------------------
// 计划来源解析接缝（issue #1090 / spec #1086 形态推广）
// ---------------------------------------------------------------------------

/// 计划来源解析钩子签名：按页收集的生成交易 id → 交易 id 到来源展示
/// （[`TransactionSource`]，本域自有模型）的映射。实现侧（定时计划域）负责把
/// `PlanSourceDisplay` 映射为本域来源模型（kind 三枚举 → 来源类型三枚举、
/// `cancelled` 状态 → Cancelled 标注，spec #704 口径零变化）。
type PlanSourceResolver = fn(&Connection, &[String]) -> Result<HashMap<String, TransactionSource>>;

/// 计划来源解析器的进程级单例（登记点反转的承接面）：先装者优先、重复注册零动作。
static PLAN_SOURCE_RESOLVER: OnceLock<PlanSourceResolver> = OnceLock::new();

/// 注册计划来源解析实现（幂等：进程级一次，重复注册保留首次实现）。
///
/// 调用点在壳层启动接线与测试建库单点（`test_support::open`、BDD world），
/// 与生产同形；实现由定时计划域提供
/// （`scheduled_transactions::source::install_plan_source_hook`），业务代码不直接调用。
///
/// 计划反查行的公开再导出已消亡，私有性由本负向用例钉死：
///
/// ```compile_fail
/// use tauri_app_lib::scheduled_transactions::source_display_by_transaction_ids;
/// ```
pub fn register_plan_source_resolver(resolver: PlanSourceResolver) {
    let _ = PLAN_SOURCE_RESOLVER.set(resolver);
}

/// 未注册错误的单一构造（纯函数，可测）：码化 Invalid——接线缺失是程序缺陷，
/// 不该被读路径静默吞掉。
fn plan_source_resolver_missing_error() -> AppError {
    AppError::coded(
        "transaction.plan-source-resolver-unregistered",
        "计划来源解析器未注册：来源列的计划反查段被跳过（壳层启动接线缺失）",
    )
}

/// 计划反查段的接缝消费点（私有）：委派给注册的实现。未注册即接线缺失，
/// 码化错误上抛（列表读取显式失败，不静默丢来源列）。
pub(crate) fn resolve_plan_sources(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<HashMap<String, TransactionSource>> {
    let resolver = PLAN_SOURCE_RESOLVER
        .get()
        .ok_or_else(plan_source_resolver_missing_error)?;
    resolver(conn, transaction_ids)
}

// ---------------------------------------------------------------------------
// 来源列①保单 / ③物品反查接缝（issue #1092，与计划反查同族同形）
// ---------------------------------------------------------------------------

/// 保单直挂反查钩子：按行内保单 id 批量反查保单档案，实现侧（保单域）把自有
/// 展示字段映射为本域来源模型（kind = Policy、展示名 = 产品名、软删 → Deleted
/// 标注，spec #704 口径零变化）。
pub type PolicySourceResolver =
    fn(&Connection, &[String]) -> Result<HashMap<String, TransactionSource>>;

/// 物品反查钩子：按溯源指针（生成交易 id）批量反查在用物品，实现侧（物品域）
/// 映射为来源模型（kind = Item、展示名 = 物品名、已处置 → Disposed 标注）。
pub type ItemSourceResolver =
    fn(&Connection, &[String]) -> Result<HashMap<String, TransactionSource>>;

static POLICY_SOURCE_RESOLVER: OnceLock<PolicySourceResolver> = OnceLock::new();
static ITEM_SOURCE_RESOLVER: OnceLock<ItemSourceResolver> = OnceLock::new();

/// 注册保单直挂反查实现（幂等：进程级一次，重复注册保留首次实现）。调用点在
/// 保单域 `install_source_hook`，壳层启动接线，业务代码不直接调用。
pub fn register_policy_source_resolver(resolver: PolicySourceResolver) {
    let _ = POLICY_SOURCE_RESOLVER.set(resolver);
}

/// 注册物品反查实现（幂等同上）。调用点在物品域 `install_source_hook`。
pub fn register_item_source_resolver(resolver: ItemSourceResolver) {
    let _ = ITEM_SOURCE_RESOLVER.set(resolver);
}

/// 保单反查委派：未注册即接线缺失，码化错误上抛（列表读取显式失败）。
pub(crate) fn resolve_policy_sources(
    conn: &Connection,
    policy_ids: &[String],
) -> Result<HashMap<String, TransactionSource>> {
    let resolver = POLICY_SOURCE_RESOLVER.get().ok_or_else(|| {
        AppError::coded(
            "transaction.policy-source-resolver-unregistered",
            "保单来源反查器未注册：来源列的保单直挂段被跳过（壳层启动接线缺失）",
        )
    })?;
    resolver(conn, policy_ids)
}

/// 物品反查委派：未注册即接线缺失，码化错误上抛（列表读取显式失败）。
pub(crate) fn resolve_item_sources(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<HashMap<String, TransactionSource>> {
    let resolver = ITEM_SOURCE_RESOLVER.get().ok_or_else(|| {
        AppError::coded(
            "transaction.item-source-resolver-unregistered",
            "物品来源反查器未注册：来源列的物品反查段被跳过（壳层启动接线缺失）",
        )
    })?;
    resolver(conn, transaction_ids)
}

#[cfg(test)]
mod tests;
