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
    let plan = behavior::plan(conn, &input, None)?;
    let row = plan.normalized_row()?;
    let id = writer::insert_row(conn, &row)?;
    behavior::apply(conn, &id, &plan)?;
    // 搜索无索引（issue #196 全量扫描实现）：写入路径零额外工作，交易立即可搜。
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
/// 整笔修改在事务内完成，校验或匹配失败回滚。事务边界内联在本函数
/// （裸 `BEGIN`/`COMMIT`/`ROLLBACK`），提交点的置脏与写时到期检查由连接层
/// 统一写入口承担（调用方经 `db.write` 执行本函数，ADR-0032）。
/// 不存在或已软删除的 id 返回 `AppError::NotFound`。
pub fn update_transaction_internal(
    conn: &Connection,
    id: &str,
    input: TransactionInput,
) -> Result<()> {
    // 读取旧交易 kind 与当前商户（商户用于「保持历史引用」判定：提交商户与原值
    // 相同则跳过在用校验，软删商户的历史交易仍可修改其他字段），不存在或已删除
    // 返回 NotFound。
    let (old_kind, old_merchant_id): (TransactionKind, Option<String>) = conn
        .query_row(
            "SELECT kind, merchant_id FROM transactions WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("交易不存在: {id}")))?;

    conn.execute("BEGIN", [])?;
    let res = (|| -> Result<()> {
        // 先按旧 kind 回退持仓/卖出关联副作用，再按新 kind 校验并应用（跨 kind 修改避免孤儿持仓）；
        // buy 守卫（已有部分卖出拒绝）措辞为「无法修改」。
        behavior::revert(conn, id, old_kind, "该买入交易已有部分卖出，无法修改")?;
        let plan = behavior::plan(conn, &input, old_merchant_id.as_deref())?;
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
