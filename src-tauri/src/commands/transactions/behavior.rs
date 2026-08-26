//! 交易类型行为层（issue #72 / spec #69 候选 2）：按 kind 收敛分派。
//!
//! 每类 kind 的行为——校验、归一化、应用副作用、回退——集中于此，对外暴露三个能力：
//! - [`plan`]：校验 + 归一化（normalize→plan，不落库、不产生副作用）；
//! - [`apply`]：应用计划的副作用（普通 kind 无副作用；buy 建仓 / sell 卖出匹配）；
//! - [`revert`]：回退一笔已存在交易（按旧 kind）的副作用（普通 kind 无副作用）。
//!
//! 分派是薄而穷尽的 `match`（不引入 trait 注册表，避免过度设计）：
//! 普通 kind（income/expense/transfer/refund）经 Writer 接缝归一化；buy/sell 委托投资域
//! （`commands::investment` 的 prepare/apply/revert，正向分派保留）；`dividend` / `split`
//! 已声明但未实现，在此显式「暂不支持」拒绝——这是本重构唯一对外的可观测行为变化
//! （此前经交易接口创建 dividend/split 落入 [`writer::normalize`] 的通用兜底，返回语义不明的
//! 「仅处理通用交易类型」；现改为明确的「暂不支持」，两者都不落库）。
//!
//! 依赖方向：命令层（transactions → investment → 无反向）。本模块与行为函数不内嵌事务、
//! 只接受连接——事务边界由调用方持有（修改路径与批量路径在编排层显式 BEGIN/COMMIT），
//! 行为层保证在这些事务内，买卖的行写入与 lot/匹配副作用同处一个事务。

use rusqlite::Connection;

use crate::commands::investment;
use crate::error::{AppError, Result};
use crate::models::TransactionInput;
use crate::transaction::amount::TransactionKind;
use crate::transaction::writer;

/// 计划：归一化后的交易行 + kind 特有副作用数据（不落库）。
pub(crate) enum Plan {
    /// 普通 kind（income/expense/transfer/refund）：无副作用。
    Common(writer::NormalizedRow),
    /// 投资 kind（buy/sell）：归一化行与副作用数据留在投资域计划中。
    Investment(investment::Plan),
}

impl Plan {
    /// 归一化交易行（供 [`writer::insert_row`] / [`writer::update_row`] 落库）。
    pub(crate) fn normalized_row(&self) -> Result<writer::NormalizedRow> {
        match self {
            Plan::Common(row) => Ok(row.clone()),
            Plan::Investment(p) => Ok(writer::NormalizedRow::try_from(p.normalized())?),
        }
    }
}

/// 校验并归一化一笔交易输入为计划（不落库、不产生副作用）。
///
/// 单点分派全部 8 种 kind：通用 kind 经 Writer 接缝 [`writer::normalize`]（金额>0、
/// transfer 目标账户、refund 继承原支出等校验 + 本位币折算）；buy/sell 委托投资域
/// [`investment::prepare`]（投资账户/数量/单价/可卖数量校验 + 折算）；
/// `dividend` / `split` 已声明但未实现，显式「暂不支持」报错——取代此前
/// [`writer::normalize`] 兜底的「仅处理通用交易类型」文案（唯一对外可观测变化）。
pub(crate) fn plan(conn: &Connection, input: &TransactionInput) -> Result<Plan> {
    let kind = input.kind;
    match kind {
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund => {
            let norm = writer::normalize(
                conn,
                &writer::Input {
                    kind,
                    amount_cents: input.amount_cents,
                    currency_code: input.currency_code.clone(),
                    account_id: input.account_id.clone(),
                    to_account_id: input.to_account_id.clone(),
                    category_id: input.category_id.clone(),
                    refund_of_transaction_id: input.refund_of_transaction_id.clone(),
                    note: input.note.clone(),
                    date: input.date.clone(),
                },
            )?;
            Ok(Plan::Common(norm))
        }
        TransactionKind::Buy | TransactionKind::Sell => {
            Ok(Plan::Investment(investment::prepare(conn, kind, input)?))
        }
        TransactionKind::Dividend | TransactionKind::Split => Err(AppError::Invalid(format!(
            "交易类型 {kind} 暂不支持（MVP 未实现）"
        ))),
    }
}

/// 应用计划的副作用（创建/修改落库后调用）。
pub(crate) fn apply(conn: &Connection, id: &str, plan: &Plan) -> Result<()> {
    match plan {
        Plan::Common(_) => Ok(()),
        Plan::Investment(p) => investment::apply(conn, id, p),
    }
}

/// 回退一笔已存在交易（按旧 kind）的副作用，供删除/修改前清理。
///
/// 普通 kind 与未实现的 dividend/split 无持仓副作用，为 no-op；
/// buy 守卫（已有部分卖出拒绝）+ 清理，sell 回补持仓扣减。
/// `partial_sold_msg` 为 buy 守卫的措辞（删除/修改场景文案不同）。
pub(crate) fn revert(
    conn: &Connection,
    id: &str,
    kind: TransactionKind,
    partial_sold_msg: &str,
) -> Result<()> {
    match kind {
        TransactionKind::Buy | TransactionKind::Sell => {
            investment::revert(conn, id, kind, partial_sold_msg)
        }
        _ => Ok(()),
    }
}
