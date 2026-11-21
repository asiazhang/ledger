use std::collections::HashMap;
use std::sync::OnceLock;

use rusqlite::Connection;

use ledger_infra::error::{AppError, Result};

use crate::model::TransactionSource;

type PlanSourceResolver = fn(&Connection, &[String]) -> Result<HashMap<String, TransactionSource>>;

static PLAN_SOURCE_RESOLVER: OnceLock<PlanSourceResolver> = OnceLock::new();

/// ```compile_fail
/// use ledger_scheduled::source_display_by_transaction_ids;
/// ```
pub fn register_plan_source_resolver(resolver: PlanSourceResolver) {
    let _ = PLAN_SOURCE_RESOLVER.set(resolver);
}

fn plan_source_resolver_missing_error() -> AppError {
    AppError::coded(
        "transaction.plan-source-resolver-unregistered",
        "计划来源解析器未注册：来源列的计划反查段被跳过（壳层启动接线缺失）",
    )
}

pub(crate) fn resolve_plan_sources(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<HashMap<String, TransactionSource>> {
    let resolver = PLAN_SOURCE_RESOLVER
        .get()
        .ok_or_else(plan_source_resolver_missing_error)?;
    resolver(conn, transaction_ids)
}

pub type PolicySourceResolver =
    fn(&Connection, &[String]) -> Result<HashMap<String, TransactionSource>>;

pub type ItemSourceResolver =
    fn(&Connection, &[String]) -> Result<HashMap<String, TransactionSource>>;

static POLICY_SOURCE_RESOLVER: OnceLock<PolicySourceResolver> = OnceLock::new();
static ITEM_SOURCE_RESOLVER: OnceLock<ItemSourceResolver> = OnceLock::new();

pub fn register_policy_source_resolver(resolver: PolicySourceResolver) {
    let _ = POLICY_SOURCE_RESOLVER.set(resolver);
}

pub fn register_item_source_resolver(resolver: ItemSourceResolver) {
    let _ = ITEM_SOURCE_RESOLVER.set(resolver);
}

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
