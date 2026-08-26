use rusqlite::Connection;
use rusqlite::OptionalExtension;

use crate::error::{AppError, Result};
use crate::models::TransactionInput;
use crate::transaction::amount::TransactionKind;
use crate::transaction::writer;

use super::behavior;

/// 创建一笔交易（`POST /api/v1/transactions/batch` 单条 / IPC `create_transaction`）。
///
/// 全部 kind 的行为收敛到行为层单点分派（issue #72）：`plan → insert_row → apply`。
/// 通用 kind（income/expense/transfer/refund）经 [`behavior::plan`] → Writer 接缝
/// [`writer::normalize`] + [`writer::insert_row`] 归一化并落库（本位币折算走 Amount
/// 接缝、id 与审计字段统一生成，与定时引擎/批量导入共用同一写入权威，列清单不在此重复）；
/// buy/sell 经投资域 prepare/apply 落交易行并建仓/卖出匹配；`dividend` / `split`
/// 已声明但未实现，在此显式「暂不支持」拒绝。
pub fn insert_transaction(conn: &Connection, input: TransactionInput) -> Result<String> {
    let plan = behavior::plan(conn, &input)?;
    let row = plan.normalized_row()?;
    let id = writer::insert_row(conn, &row)?;
    behavior::apply(conn, &id, &plan)?;
    // 索引维护由后台定时刷新承担（ADR-0004 决策 #14）：触发器已入队
    // `search_reindex_queue`，写路径不做任何同步索引工作（界面操作零索引开销）。
    Ok(id)
}

/// 按 `id` 全字段替换一笔交易（`PUT /api/v1/transactions/{id}`）。
///
/// 行为收敛到行为层单点分派（issue #72）：先按旧 kind 回退持仓/卖出关联副作用，
/// 再按新 kind 校验归一化（`behavior::plan`）、经 Writer 接缝 [`writer::update_row`]
/// 落交易行字段并应用新副作用——跨 kind 修改（如 expense→buy）避免孤儿持仓。
/// 幂等键（`idempotency_key`）与内容哈希（`dedup_hash`）不作为
/// 可编辑字段——修改不重算去重身份，故修改后重跑同批导入（带幂等键）仍按同键去重、不产生重复。
///
/// 整笔修改在事务内完成，校验或匹配失败回滚。
/// 不存在或已软删除的 id 返回 `AppError::NotFound`。
pub fn update_transaction_internal(
    conn: &Connection,
    id: &str,
    input: TransactionInput,
) -> Result<()> {
    // 读取旧交易 kind，用于按旧 kind 回退持仓/卖出关联；不存在或已删除返回 NotFound。
    let old_kind_str: String = conn
        .query_row(
            "SELECT kind FROM transactions WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("交易不存在: {id}")))?;
    let old_kind = TransactionKind::parse(&old_kind_str)?;

    conn.execute("BEGIN", [])?;
    let res = (|| -> Result<()> {
        // 先按旧 kind 回退持仓/卖出关联副作用，再按新 kind 校验并应用（跨 kind 修改避免孤儿持仓）；
        // buy 守卫（已有部分卖出拒绝）措辞为「无法修改」。
        behavior::revert(conn, id, old_kind, "该买入交易已有部分卖出，无法修改")?;
        let plan = behavior::plan(conn, &input)?;
        let row = plan.normalized_row()?;
        writer::update_row(conn, id, &row)?;
        behavior::apply(conn, id, &plan)
    })();
    match res {
        Ok(()) => {
            conn.execute("COMMIT", [])?;
            Ok(())
        }
        Err(e) => {
            conn.execute("ROLLBACK", [])?;
            Err(e)
        }
    }
}
