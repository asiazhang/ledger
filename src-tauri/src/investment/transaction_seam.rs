//! 交易域接缝实现（spec #1086 / issue #1092）：投资 kind 写路径装配/副作用与读
//! 路径投影的实现注册面。
//!
//! 注册点住核心交易域（`transaction::seams::investment`，计划契约
//! [`InvestmentPlan`](crate::transaction::seams::investment::InvestmentPlan) 与命令
//! 字段类型均为其自有）；本模块把投资域自有计划（[`Plan`]）适配进契约、把四个
//! 写路径挂载点（Local 装配 / Replay 装配 / 修改回退 / 删除释放）与两个读路径
//! 投影（来源列④标的反查、转换两腿扩展）经 `install_transaction_hooks` 一次性
//! 注册，壳层启动接线（下层定义注册点、上层注册实现，ADR-0112 决策 5）——
//! `transaction → investment` 直接依赖边随 #1092 消亡，依赖方向收敛为
//! investment → transaction 单向。
//!
//! 命令字段摘取（计划变体 → 命令字段）与重放语义字段缺省的防御臂自 behavior
//! 随接缝迁入：错误码与文案逐字保留（`transaction.convert-fields-missing` /
//! `transaction.split-fields-missing`），行为零变化。

use std::collections::HashMap;

use rusqlite::Connection;

use super::source as instrument_source;
use super::trade::{self, Plan, replay_convert_plan, replay_plan, replay_split_plan};
use crate::error::{AppError, Result};
use crate::transaction::amount::TransactionKind;
use crate::transaction::command::{
    ConvertCommandFields, InvestmentCommandFields, SplitCommandFields,
};
use crate::transaction::seams::investment::{
    ConvertFieldsResolver, InstrumentSourceResolver, PlanCommandParts, PrepareHook,
    ReleaseForDeleteHook, ReplayAssemblyHook, RevertHook,
};
use crate::transaction::seams::investment::{
    register_convert_fields_resolver, register_instrument_source_resolver, register_prepare_hook,
    register_release_for_delete_hook, register_replay_hook, register_revert_hook,
};
use crate::transaction::{
    ConvertFields, NormalizedTransaction, TransactionInput, TransactionSource,
    TransactionSourceKind,
};

// ---------------------------------------------------------------------------
// 计划契约适配（InvestmentPlan for Plan）
// ---------------------------------------------------------------------------

impl crate::transaction::seams::investment::InvestmentPlan for Plan {
    fn normalized(&self) -> &NormalizedTransaction {
        Plan::normalized(self)
    }

    /// 命令字段部件（产出侧桥，自 behavior::command_parts 的投资臂迁入）：
    /// 按计划变体摘取语义字段；份额调整、转换走各自独立的可选成员（investment
    /// 恒 None）——语义字段与 buy/sell 不同构，不塞进同一结构（ADR-0106 决策 9 /
    /// ADR-0099 决策 6）。
    fn command_parts(&self) -> PlanCommandParts {
        let fields = match self {
            Plan::Buy(b) => Some(InvestmentCommandFields {
                instrument_id: b.instrument_id.clone(),
                quantity: b.quantity,
                price_cents: b.price_cents,
                fee_cents: b.fee_cents,
                cost_per_unit_cents: Some(b.cost_per_unit_cents),
            }),
            Plan::Sell(s) => Some(InvestmentCommandFields {
                instrument_id: s.instrument_id.clone(),
                quantity: s.quantity,
                price_cents: s.price_cents,
                fee_cents: s.fee_cents,
                cost_per_unit_cents: None,
            }),
            // 现金分红（issue #1078 / ADR-0109）：语义字段只有标的 id，其余成员
            // 是占位（分红无份额 / 单价 / 手续费，行金额随归一化行携带）——重放端
            // 只读 `instrument_id`，其余不消费；载荷形状对齐 buy/sell，不新增结构。
            Plan::Dividend(d) => Some(InvestmentCommandFields {
                instrument_id: d.instrument_id.clone(),
                quantity: 0.0,
                price_cents: 0,
                fee_cents: 0,
                cost_per_unit_cents: None,
            }),
            Plan::Split(_) | Plan::Convert(_) => None,
        };
        // 份额调整的比对锚点（ADR-0106 决策 9）：Δ + 源端最终持仓 + 批次总成本。
        let split = match self {
            Plan::Split(s) => Some(SplitCommandFields {
                instrument_id: s.instrument_id.clone(),
                delta_quantity: s.delta_quantity,
                final_quantity: s.final_quantity,
                total_cost_cents: s.total_cost_cents,
            }),
            Plan::Buy(_) | Plan::Sell(_) | Plan::Dividend(_) | Plan::Convert(_) => None,
        };
        let convert = match self {
            Plan::Convert(c) => Some(ConvertCommandFields {
                instrument_id: c.instrument_id.clone(),
                quantity: c.quantity,
                to_instrument_id: c.to_instrument_id.clone(),
                to_quantity: c.to_quantity,
                out_amount_cents: c.out_amount_cents,
                in_amount_cents: c.in_amount_cents,
                fee_cents: c.fee_cents,
                carried_cost_cents: c.carried_cost_cents(),
            }),
            Plan::Buy(_) | Plan::Sell(_) | Plan::Dividend(_) | Plan::Split(_) => None,
        };
        PlanCommandParts {
            investment: fields,
            split,
            convert,
        }
    }

    /// 应用计划副作用（创建/修改落库后调用，与行写入同事务）——委托
    /// [`trade::apply`]（既有实现，语义零变化）。
    fn apply(&self, conn: &Connection, transaction_id: &str) -> Result<()> {
        trade::apply(conn, transaction_id, self)
    }
}

// ---------------------------------------------------------------------------
// 写路径挂载点实现
// ---------------------------------------------------------------------------

/// Local 装配实现：委托 [`trade::prepare`]（校验 + 折算 + 副作用数据算定）。
fn prepare_hook(
    conn: &Connection,
    kind: TransactionKind,
    input: &TransactionInput,
    existing_id: Option<&str>,
) -> Result<Box<dyn crate::transaction::seams::investment::InvestmentPlan>> {
    Ok(Box::new(trade::prepare(conn, kind, input, existing_id)?))
}

/// 投资命令字段解包（防御臂，自 behavior 迁入逐字保留）：buy/sell/dividend 命令
/// 必携投资字段；缺失属载荷伪造或程序缺陷（产出侧永不产 None 的投资命令），
/// fail loud 由引擎挂起承接。
fn investment_fields(
    investment: Option<&InvestmentCommandFields>,
) -> Result<&InvestmentCommandFields> {
    investment.ok_or_else(|| AppError::Invalid("投资命令缺少投资字段（程序缺陷）".into()))
}

/// 转换命令字段解包（防御臂，自 behavior 迁入逐字保留）：convert 命令必携转换
/// 字段；缺失属旧版本设备载荷或程序缺陷（本地 plan 自 #980 起恒产出），码化失败
/// 由引擎挂起承接（ADR-0099 决策 6 的 kind 防御臂——旧端 op 与 schema 版本硬检查
/// 双保险）。
fn convert_fields(convert: Option<&ConvertCommandFields>) -> Result<&ConvertCommandFields> {
    convert.ok_or_else(|| {
        AppError::coded(
            "transaction.convert-fields-missing",
            "该转换操作缺少同步所需的转换字段（产生自较早版本），无法在本机重放；请在来源设备上删除并重新录入该转换后再次同步",
        )
    })
}

/// 份额调整命令字段解包（防御臂，自 behavior 迁入逐字保留）：split 命令必携份额
/// 调整字段；缺失属旧版本设备载荷或程序缺陷（本地 plan 自 #1053 起恒产出），码化
/// 失败由引擎挂起承接（与 convert 的 kind 防御臂同规——旧端 op 不携比对锚点，
/// 重放端无从校验重述，宁可挂起也不静默落出未经校验的批次成本）。
fn split_fields(split: Option<&SplitCommandFields>) -> Result<&SplitCommandFields> {
    split.ok_or_else(|| {
        AppError::coded(
            "transaction.split-fields-missing",
            "该份额调整操作缺少同步所需的份额调整字段（产生自较早版本），无法在本机重放；请在来源设备上删除并重新录入该份额调整后再次同步",
        )
    })
}

/// Replay 装配实现：按 kind 分派到投资域三个重放装配原语（自 behavior
/// replay_assembly 的投资臂迁入；普通 kind 不经本钩子——行为层已按 kind 分流，
/// 防御臂防编排错误）。
fn replay_hook(
    conn: &Connection,
    row: &NormalizedTransaction,
    existing_id: Option<&str>,
    investment: Option<&InvestmentCommandFields>,
    convert: Option<&ConvertCommandFields>,
    split: Option<&SplitCommandFields>,
) -> Result<Box<dyn crate::transaction::seams::investment::InvestmentPlan>> {
    match row.kind {
        TransactionKind::Buy | TransactionKind::Sell | TransactionKind::Dividend => {
            let fields = investment_fields(investment)?;
            Ok(Box::new(replay_plan(conn, row.kind, row, fields)?))
        }
        TransactionKind::Convert => {
            let fields = convert_fields(convert)?;
            Ok(Box::new(replay_convert_plan(conn, row, fields)?))
        }
        TransactionKind::Split => {
            let fields = split_fields(split)?;
            Ok(Box::new(replay_split_plan(conn, row, fields, existing_id)?))
        }
        // 行为层重放入口穷尽分派保证仅转发投资 kind；其余 kind 属编排错误，
        // 显式拒绝防误用（与 prepare 的防御臂同款）。
        kind @ (TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund) => Err(AppError::Invalid(format!(
            "投资层仅处理 buy/sell/convert/split/dividend，收到: {kind}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// 读路径投影实现
// ---------------------------------------------------------------------------

/// 来源列④标的反查实现：委托 [`instrument_source::source_display_by_transaction_ids`]
/// 并映射为本域来源模型（kind = Instrument、entity = 标的 id、展示名 =
/// `display_label()` 代码 + 名称空格连接；标的字典无软删，恒无状态标注——口径
/// 零变化，spec #704）。
fn instrument_source_resolver(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<HashMap<String, TransactionSource>> {
    let rows = instrument_source::source_display_by_transaction_ids(conn, transaction_ids)?;
    Ok(rows
        .into_iter()
        .map(|row| {
            (
                row.transaction_id.clone(),
                TransactionSource {
                    kind: TransactionSourceKind::Instrument,
                    entity_id: row.instrument_id.clone(),
                    display_name: row.display_label(),
                    status: None,
                },
            )
        })
        .collect())
}

/// 转换两腿扩展实现：委托 [`trade::convert_fields_by_transaction_ids`]，键为
/// 交易 id（一转换至多一行）。
fn convert_fields_resolver(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<HashMap<String, ConvertFields>> {
    Ok(
        trade::convert_fields_by_transaction_ids(conn, transaction_ids)?
            .into_iter()
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// 接线入口（壳层启动 / 测试建库单点 / BDD world / ledger-perf 调用）
// ---------------------------------------------------------------------------

/// 注册本域对交易域接缝的全部实现（幂等：各槽进程级一次，重复注册保留首次）。
///
/// 覆盖六个挂载点：Local 装配 / Replay 装配 / 修改回退 / 删除释放（写路径）+
/// 来源列④标的反查 / 转换两腿扩展（读路径）。
pub fn install_transaction_hooks() {
    let prepare: PrepareHook = prepare_hook;
    let replay: ReplayAssemblyHook = replay_hook;
    let revert: RevertHook = trade::revert;
    let release: ReleaseForDeleteHook = trade::release_for_delete;
    register_prepare_hook(prepare);
    register_replay_hook(replay);
    register_revert_hook(revert);
    register_release_for_delete_hook(release);
    let instrument: InstrumentSourceResolver = instrument_source_resolver;
    let convert: ConvertFieldsResolver = convert_fields_resolver;
    register_instrument_source_resolver(instrument);
    register_convert_fields_resolver(convert);
}
