//! 交易×投资接缝（跨域接缝区，issue #1092）：投资 kind 装配/副作用/投影注册点。
//!
//! 职责：本域只承诺调用时机与契约类型（[`InvestmentPlan`] + 三份命令字段），投资
//! 语义（标的/持仓/FIFO/批次重述校验与落库）全留投资域实现侧。不变量：未注册即
//! 码化错误（写入/读取显式失败，不静默丢装配或投影）；核心交易 crate 对任何业务域
//! 零依赖，本模块是「投资域 → 核心交易域」单向边的注册面。ADR 指针：ADR-0112
//! 决策 5 / ADR-0113 决策 8。陷阱：写路径注册点（装配/回退/释放）与读路径投影注册点
//! （标的反查/转换两腿）同住本模块；实现经 `install_transaction_hooks` 装入。

use std::collections::HashMap;
use std::sync::OnceLock;

use rusqlite::Connection;

use crate::command::{ConvertCommandFields, InvestmentCommandFields, SplitCommandFields};
use crate::model::{ConvertFields, NormalizedTransaction, TransactionInput, TransactionSource};
use ledger_infra::error::{AppError, Result};

use crate::amount::TransactionKind;

// ---------------------------------------------------------------------------
// 计划契约：投资域实现、本域消费的计划对象接口
// ---------------------------------------------------------------------------

/// 计划的命令字段部件（产出侧桥的载荷，本域自有命令字段类型）：三个成员一一
/// 对应 [`crate::command::TransactionCommand`] Create/Update 的三个可选语义字段。
///
/// 归集为结构体（而非四元组）使 [`InvestmentPlan::command_parts`] 单方法交付；
/// 普通 kind 恒为 [`PlanCommandParts::none`]，投资 kind 由实现侧按计划变体填充。
#[derive(Debug, Clone, PartialEq)]
pub struct PlanCommandParts {
    /// 投资 kind（buy/sell/dividend）语义字段。
    pub investment: Option<InvestmentCommandFields>,
    /// 份额调整（split）语义字段与源端比对锚点。
    pub split: Option<SplitCommandFields>,
    /// 转换（convert）语义字段与源端结转成本。
    pub convert: Option<ConvertCommandFields>,
}

impl PlanCommandParts {
    /// 全 None（普通 kind 专用构造，语义字段缺省）。
    pub fn none() -> Self {
        Self {
            investment: None,
            split: None,
            convert: None,
        }
    }
}

/// 投资计划契约（本域定义、投资域实现）：一笔投资 kind 交易「落库后如何应用副作用、
/// 产出命令携带什么」的抽象——计划对象由投资域在装配（[`PrepareHook`] /
/// [`ReplayAssemblyHook`]）时构造，随行为层协议流转到落库（归一化行）与副作用应用
/// （[`InvestmentPlan::apply`]）与 op 产出（命令字段）。
///
/// 契约类型全部为本域自有（[`NormalizedTransaction`] / 三份命令字段），投资域
/// 实现把自有计划数据适配进契约——核心交易域对投资域计划内部（批次消耗规划、
/// 重述快照等）零感知。
pub trait InvestmentPlan {
    /// 归一化交易行（供 `writer::NormalizedRow` 落库与命令行携带）。
    fn normalized(&self) -> &NormalizedTransaction;

    /// 命令字段部件（op 产出侧桥）：按计划 kind 交付语义字段，普通 kind 恒 none。
    fn command_parts(&self) -> PlanCommandParts;

    /// 应用计划副作用（创建/修改落库后调用，与行写入同事务）。
    fn apply(&self, conn: &Connection, transaction_id: &str) -> Result<()>;
}

// ---------------------------------------------------------------------------
// 写路径注册点：装配（Local / Replay）、修改回退、删除释放
// ---------------------------------------------------------------------------

/// 投资计划装配钩子（Local 形态）：按输入装配投资 kind 计划（校验 + 折算 +
/// 副作用数据算定，不落库不产副作用）。实现由投资域提供。
pub type PrepareHook = fn(
    &Connection,
    TransactionKind,
    &TransactionInput,
    Option<&str>,
) -> Result<Box<dyn InvestmentPlan>>;

/// 投资计划装配钩子（Replay 形态）：从命令携带的归一化行与语义字段装配计划，
/// 不重折算（源端折算，ADR-0091 决策 3）；依赖在位校验（标的存在、投资账户、
/// 可卖数量、结转成本/总量比对）以与本地写入同码的码化错误上抛。语义字段缺省
/// 的防御臂（伪造/旧版本载荷 → 挂起）同归实现侧，错误码与文案与迁移前逐字一致。
pub type ReplayAssemblyHook = fn(
    &Connection,
    &NormalizedTransaction,
    Option<&str>,
    Option<&InvestmentCommandFields>,
    Option<&ConvertCommandFields>,
    Option<&SplitCommandFields>,
) -> Result<Box<dyn InvestmentPlan>>;

/// 持仓副作用回退钩子（修改路径清理阶段）：按旧 kind 回退 buy/sell/convert 的
/// 持仓与卖出关联副作用（含在用占用/转换链守卫），普通 kind 无副作用（no-op）。
pub type RevertHook = fn(&Connection, &str, TransactionKind) -> Result<()>;

/// 删除释放钩子（删除编排）：sell 回补 / buy 与 convert 级联消化在用 sell /
/// split 按重述审计精确回补 / dividend 摘除扩展行，返回被级联软删的 sell id 列表
/// （行为层逐笔软删并各自产 delete op 与余额刷新）。
pub type ReleaseForDeleteHook = fn(&Connection, &str, TransactionKind) -> Result<Vec<String>>;

static PREPARE_HOOK: OnceLock<PrepareHook> = OnceLock::new();
static REPLAY_HOOK: OnceLock<ReplayAssemblyHook> = OnceLock::new();
static REVERT_HOOK: OnceLock<RevertHook> = OnceLock::new();
static RELEASE_FOR_DELETE_HOOK: OnceLock<ReleaseForDeleteHook> = OnceLock::new();

/// 注册投资计划装配实现（Local 形态；幂等：进程级一次，重复注册保留首次）。
/// 调用点在投资域 `install_transaction_hooks`，壳层启动接线，业务代码不直接调用。
pub fn register_prepare_hook(hook: PrepareHook) {
    let _ = PREPARE_HOOK.set(hook);
}

/// 注册投资重放装配实现（Replay 形态；幂等同上）。
pub fn register_replay_hook(hook: ReplayAssemblyHook) {
    let _ = REPLAY_HOOK.set(hook);
}

/// 注册持仓副作用回退实现（幂等同上）。
pub fn register_revert_hook(hook: RevertHook) {
    let _ = REVERT_HOOK.set(hook);
}

/// 注册删除释放实现（幂等同上）。
pub fn register_release_for_delete_hook(hook: ReleaseForDeleteHook) {
    let _ = RELEASE_FOR_DELETE_HOOK.set(hook);
}

/// 未注册错误的单一构造（纯函数，可测）。
fn investment_hook_missing_error(slot: &'static str) -> AppError {
    AppError::coded(
        "transaction.investment-hook-unregistered",
        format!("投资接缝钩子未注册（{slot}）：投资 kind 的交易写入被拒绝（壳层启动接线缺失）"),
    )
}

/// 装配委派（Local 形态）：钩子在场即调用，缺席即码化错误。
pub(crate) fn prepare_investment(
    conn: &Connection,
    kind: TransactionKind,
    input: &TransactionInput,
    existing_id: Option<&str>,
) -> Result<Box<dyn InvestmentPlan>> {
    let hook = PREPARE_HOOK
        .get()
        .ok_or_else(|| investment_hook_missing_error("prepare"))?;
    hook(conn, kind, input, existing_id)
}

/// 装配委派（Replay 形态）：钩子在场即调用，缺席即码化错误。
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

/// 回退委派（修改路径）：钩子在场即调用，缺席即码化错误（修改随事务回滚）。
pub(crate) fn revert_investment(conn: &Connection, id: &str, kind: TransactionKind) -> Result<()> {
    let hook = REVERT_HOOK
        .get()
        .ok_or_else(|| investment_hook_missing_error("revert"))?;
    hook(conn, id, kind)
}

/// 释放委派（删除编排）：钩子在场即调用，缺席即码化错误（删除随事务回滚）。
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

// ---------------------------------------------------------------------------
// 读路径注册点：来源列④标的反查 + 转换两腿扩展
// ---------------------------------------------------------------------------

/// 来源列④标的反查钩子：按生成交易 id 批量反查证券交易记录指向的标的，实现侧
/// （投资域）负责把自有展示字段映射为本域来源模型（[`TransactionSource`]，
/// 展示名 = 代码 + 名称空格连接、无状态标注——口径零变化，spec #704）。
pub type InstrumentSourceResolver =
    fn(&Connection, &[String]) -> Result<HashMap<String, TransactionSource>>;

/// 转换两腿扩展钩子（ADR-0099）：按交易 id 批量取转换明细，实现侧负责组装本域
/// [`ConvertFields`]（行金额锚点是结转成本，列表金额列展示转出金额须读本扩展）。
pub type ConvertFieldsResolver =
    fn(&Connection, &[String]) -> Result<HashMap<String, ConvertFields>>;

static INSTRUMENT_SOURCE_RESOLVER: OnceLock<InstrumentSourceResolver> = OnceLock::new();
static CONVERT_FIELDS_RESOLVER: OnceLock<ConvertFieldsResolver> = OnceLock::new();

/// 注册标的来源反查实现（幂等：进程级一次，重复注册保留首次）。
pub fn register_instrument_source_resolver(resolver: InstrumentSourceResolver) {
    let _ = INSTRUMENT_SOURCE_RESOLVER.set(resolver);
}

/// 注册转换两腿扩展实现（幂等同上）。
pub fn register_convert_fields_resolver(resolver: ConvertFieldsResolver) {
    let _ = CONVERT_FIELDS_RESOLVER.set(resolver);
}

/// 来源列④委派：未注册即接线缺失，码化错误上抛（列表读取显式失败，不静默丢来源列）。
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

/// 转换两腿委派：未注册即接线缺失，码化错误上抛（列表读取显式失败）。
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
mod tests {
    use super::*;

    #[test]
    fn 钩子未注册即码化错误_错误码可判读() {
        // 建库经统一测试工厂（ADR-0084 规则 1）；工厂顺带注册的全局钩子在场，
        // 本断言只对准错误码构造纯函数（不读槽），与全局状态无涉。
        let err = investment_hook_missing_error("prepare");
        assert_eq!(err.code(), Some("transaction.investment-hook-unregistered"));
    }

    #[test]
    fn 命令字段部件全缺省构造_none() {
        let parts = PlanCommandParts::none();
        assert!(parts.investment.is_none());
        assert!(parts.split.is_none());
        assert!(parts.convert.is_none());
    }
}
