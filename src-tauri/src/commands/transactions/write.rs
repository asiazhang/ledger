use rusqlite::Connection;
use rusqlite::OptionalExtension;

use crate::commands::fx::convert_to_native;
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{CreateTransactionResult, NormalizedTransaction, TransactionInput};

pub fn insert_transaction(conn: &Connection, input: TransactionInput) -> Result<String> {
    let id = if input.kind == "buy" {
        crate::commands::investment::create_buy_transaction(conn, input)?
    } else if input.kind == "sell" {
        crate::commands::investment::create_sell_transaction(conn, input)?
    } else {
        let norm = normalize_transaction(conn, &input)?;
        let id = new_uuid();
        let now = now_iso();
        conn.execute(
            "INSERT INTO transactions \
             (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
             category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,0)",
            rusqlite::params![
                id,
                norm.kind,
                norm.amount_cents,
                norm.currency_code,
                norm.amount_native_cents,
                norm.account_id,
                norm.to_account_id,
                norm.category_id,
                norm.refund_of_transaction_id,
                norm.note,
                norm.date,
                now,
                now,
                1,
                device_id()
            ],
        )?;
        id
    };
    // 索引维护由后台定时刷新（ADR-0004 决策 #14）承担：触发器已入队
    // `search_reindex_queue`，写路径不做任何同步索引工作（界面操作零索引开销）。
    Ok(id)
}

/// 校验并按 kind 归一化交易字段，产出可直接 INSERT/UPDATE 的交易行字段。
///
/// 创建与修改共用：只做校验与字段解析，不做任何落库——buy/sell 的持仓建仓、
/// 卖出匹配等副作用由调用方在落库时按其身份（新增或替换）另行执行。
/// 转账需 `to_account_id`、退款需关联未删除的支出交易、买入/卖出需投资账户与标的校验。
pub fn normalize_transaction(
    conn: &Connection,
    input: &TransactionInput,
) -> Result<NormalizedTransaction> {
    if input.kind == "transfer" && input.to_account_id.is_none() {
        return Err(AppError::Invalid("转账必须指定目标账户".into()));
    }
    match input.kind.as_str() {
        "buy" => crate::commands::investment::prepare_buy(conn, input).map(|p| p.normalized),
        "sell" => crate::commands::investment::prepare_sell(conn, input).map(|p| p.normalized),
        _ => {
            if input.amount_cents <= 0 {
                return Err(AppError::Invalid("金额必须大于 0".into()));
            }
            let (category_id, account_id, currency_code, refund_of_id) = if input.kind == "refund" {
                let ref_id = input
                    .refund_of_transaction_id
                    .clone()
                    .ok_or_else(|| AppError::Invalid("退款必须关联原支出交易".into()))?;
                let (cat, acc, cur, okind): (Option<String>, String, String, String) = conn
                    .query_row(
                        "SELECT category_id, account_id, currency_code, kind \
                         FROM transactions WHERE id=?1 AND is_deleted=0",
                        rusqlite::params![ref_id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                    )?;
                if okind != "expense" {
                    return Err(AppError::Invalid("退款只能关联支出交易".into()));
                }
                (cat, acc, cur, Some(ref_id))
            } else {
                (
                    input.category_id.clone(),
                    input.account_id.clone(),
                    input.currency_code.clone(),
                    None,
                )
            };
            let native = convert_to_native(conn, input.amount_cents, &currency_code, &account_id)?;
            let to_account_id = if input.kind == "transfer" {
                input.to_account_id.clone()
            } else {
                None
            };
            Ok(NormalizedTransaction {
                kind: input.kind.clone(),
                amount_cents: input.amount_cents,
                currency_code,
                amount_native_cents: native,
                account_id,
                to_account_id,
                category_id,
                refund_of_transaction_id: refund_of_id,
                note: input.note.clone(),
                date: input.date.clone(),
            })
        }
    }
}

/// 薄转发层（issue #63）：批量编排语义已整体迁入 `batch::TransactionBatch::run`，
/// 本函数保留以兼容既有调用点——HTTP handler、IPC `create_transactions`、
/// 搜索重建测试与 e2e 迁移 step——对外契约（返回形状/事务/去重语义）完全不变。
pub fn create_transactions_internal(
    conn: &Connection,
    inputs: Vec<TransactionInput>,
    dedup: bool,
) -> Result<Vec<CreateTransactionResult>> {
    crate::commands::batch::TransactionBatch::run(conn, inputs, dedup)
}

/// 按 `id` 全字段替换一笔交易（`PUT /api/v1/transactions/{id}`）。
///
/// 复用 `normalize_transaction`（其 buy/sell 分支复用 `prepare_buy`/`prepare_sell`）校验并归一化
/// 字段，关联约束与创建路径一致。幂等键（`idempotency_key`）与内容哈希（`dedup_hash`）不作为
/// 可编辑字段——修改不重算去重身份，故修改后重跑同批导入（带幂等键）仍按同键去重、不产生重复。
///
/// buy/sell 的持仓/卖出关联在各自替换路径处理（先按旧 kind 清理/回补，再按新 kind 重建），
/// 跨 kind 修改（如 expense→buy）避免孤儿持仓。整笔修改在事务内完成，校验或匹配失败回滚。
/// 不存在或已软删除的 id 返回 `AppError::NotFound`。
pub fn update_transaction_internal(
    conn: &Connection,
    id: &str,
    input: TransactionInput,
) -> Result<()> {
    // 读取旧交易 kind，用于按旧 kind 清理持仓/卖出关联；不存在或已删除返回 NotFound。
    let old_kind: String = conn
        .query_row(
            "SELECT kind FROM transactions WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("交易不存在: {id}")))?;

    conn.execute("BEGIN", [])?;
    let res = (|| -> Result<()> {
        // 先按旧 kind 清理/回补持仓副作用，再按新 kind 校验并应用（跨 kind 修改避免孤儿持仓）。
        match old_kind.as_str() {
            "buy" => crate::commands::investment::cleanup_buy(conn, id)?,
            "sell" => crate::commands::investment::reverse_sell(conn, id)?,
            _ => {}
        }
        match input.kind.as_str() {
            "buy" => crate::commands::investment::apply_buy(conn, id, &input),
            "sell" => crate::commands::investment::apply_sell(conn, id, &input),
            _ => {
                let norm = normalize_transaction(conn, &input)?;
                super::update_transaction_row(conn, id, &norm)?;
                Ok(())
            }
        }
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
