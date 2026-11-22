use std::collections::HashMap;
use std::sync::OnceLock;

use rusqlite::Connection;

use crate::command::{ConvertCommandFields, InvestmentCommandFields, SplitCommandFields};
use crate::model::{ConvertFields, NormalizedTransaction, TransactionInput, TransactionSource};
use ledger_infra::error::{AppError, Result};

use crate::amount::{FxEditBaseline, TransactionKind};

#[derive(Debug, Clone, PartialEq)]
pub struct PlanCommandParts {
    pub investment: Option<InvestmentCommandFields>,
    pub split: Option<SplitCommandFields>,
    pub convert: Option<ConvertCommandFields>,
}

impl PlanCommandParts {
    pub fn none() -> Self {
        Self {
            investment: None,
            split: None,
            convert: None,
        }
    }
}

pub trait InvestmentPlan {
    fn normalized(&self) -> &NormalizedTransaction;

    fn command_parts(&self) -> PlanCommandParts;

    fn apply(&self, conn: &Connection, transaction_id: &str) -> Result<()>;
}

pub type PrepareHook = fn(
    &Connection,
    TransactionKind,
    &TransactionInput,
    Option<&str>,
    Option<&FxEditBaseline>,
) -> Result<Box<dyn InvestmentPlan>>;

pub type ReplayAssemblyHook = fn(
    &Connection,
    &NormalizedTransaction,
    Option<&str>,
    Option<&InvestmentCommandFields>,
    Option<&ConvertCommandFields>,
    Option<&SplitCommandFields>,
) -> Result<Box<dyn InvestmentPlan>>;

pub type RevertHook = fn(&Connection, &str, TransactionKind) -> Result<()>;

pub type ReleaseForDeleteHook = fn(&Connection, &str, TransactionKind) -> Result<Vec<String>>;

static PREPARE_HOOK: OnceLock<PrepareHook> = OnceLock::new();
static REPLAY_HOOK: OnceLock<ReplayAssemblyHook> = OnceLock::new();
static REVERT_HOOK: OnceLock<RevertHook> = OnceLock::new();
static RELEASE_FOR_DELETE_HOOK: OnceLock<ReleaseForDeleteHook> = OnceLock::new();

pub fn register_prepare_hook(hook: PrepareHook) {
    let _ = PREPARE_HOOK.set(hook);
}

pub fn register_replay_hook(hook: ReplayAssemblyHook) {
    let _ = REPLAY_HOOK.set(hook);
}

pub fn register_revert_hook(hook: RevertHook) {
    let _ = REVERT_HOOK.set(hook);
}

pub fn register_release_for_delete_hook(hook: ReleaseForDeleteHook) {
    let _ = RELEASE_FOR_DELETE_HOOK.set(hook);
}

fn investment_hook_missing_error(slot: &'static str) -> AppError {
    AppError::coded(
        "transaction.investment-hook-unregistered",
        format!("投资接缝钩子未注册（{slot}）：投资 kind 的交易写入被拒绝（壳层启动接线缺失）"),
    )
}

pub(crate) fn prepare_investment(
    conn: &Connection,
    kind: TransactionKind,
    input: &TransactionInput,
    existing_id: Option<&str>,
    fx_edit_baseline: Option<&FxEditBaseline>,
) -> Result<Box<dyn InvestmentPlan>> {
    let hook = PREPARE_HOOK
        .get()
        .ok_or_else(|| investment_hook_missing_error("prepare"))?;
    hook(conn, kind, input, existing_id, fx_edit_baseline)
}

pub(crate) fn replay_investment(
    conn: &Connection,
    row: &NormalizedTransaction,
    existing_id: Option<&str>,
    investment: Option<&InvestmentCommandFields>,
    convert: Option<&ConvertCommandFields>,
    split: Option<&SplitCommandFields>,
) -> Result<Box<dyn InvestmentPlan>> {
    let hook = REPLAY_HOOK
        .get()
        .ok_or_else(|| investment_hook_missing_error("replay"))?;
    hook(conn, row, existing_id, investment, convert, split)
}

pub(crate) fn revert_investment(conn: &Connection, id: &str, kind: TransactionKind) -> Result<()> {
    let hook = REVERT_HOOK
        .get()
        .ok_or_else(|| investment_hook_missing_error("revert"))?;
    hook(conn, id, kind)
}

pub(crate) fn release_for_delete_investment(
    conn: &Connection,
    id: &str,
    kind: TransactionKind,
) -> Result<Vec<String>> {
    let hook = RELEASE_FOR_DELETE_HOOK
        .get()
        .ok_or_else(|| investment_hook_missing_error("release_for_delete"))?;
    hook(conn, id, kind)
}

pub type InstrumentSourceResolver =
    fn(&Connection, &[String]) -> Result<HashMap<String, TransactionSource>>;

pub type ConvertFieldsResolver =
    fn(&Connection, &[String]) -> Result<HashMap<String, ConvertFields>>;

static INSTRUMENT_SOURCE_RESOLVER: OnceLock<InstrumentSourceResolver> = OnceLock::new();
static CONVERT_FIELDS_RESOLVER: OnceLock<ConvertFieldsResolver> = OnceLock::new();

pub fn register_instrument_source_resolver(resolver: InstrumentSourceResolver) {
    let _ = INSTRUMENT_SOURCE_RESOLVER.set(resolver);
}

pub fn register_convert_fields_resolver(resolver: ConvertFieldsResolver) {
    let _ = CONVERT_FIELDS_RESOLVER.set(resolver);
}

pub(crate) fn resolve_instrument_sources(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<HashMap<String, TransactionSource>> {
    let resolver = INSTRUMENT_SOURCE_RESOLVER.get().ok_or_else(|| {
        AppError::coded(
            "transaction.instrument-source-resolver-unregistered",
            "标的来源反查器未注册：来源列的标的反查段被跳过（壳层启动接线缺失）",
        )
    })?;
    resolver(conn, transaction_ids)
}

pub(crate) fn resolve_convert_fields(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<HashMap<String, ConvertFields>> {
    let resolver = CONVERT_FIELDS_RESOLVER.get().ok_or_else(|| {
        AppError::coded(
            "transaction.convert-fields-resolver-unregistered",
            "转换字段反查器未注册：转换两腿扩展被跳过（壳层启动接线缺失）",
        )
    })?;
    resolver(conn, transaction_ids)
}

#[cfg(test)]
mod tests;
